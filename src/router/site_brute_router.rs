/* Copyright (C) 2022 Antmicro
 * 
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 * 
 *     https://www.apache.org/licenses/LICENSE-2.0
 * 
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */


use std::borrow::Borrow;
use core::panic;
use std::collections::{HashMap, VecDeque};
use crate::common::{
    IcStr,
    split_range_nicely
};
use crate::logic_formula::*;
use lazy_static::__Deref;
use replace_with::replace_with_or_abort;
use std::thread;
use std::sync::{Arc, Mutex};
#[allow(unused)]
use crate::log::*;
use crate::ic_loader::archdef::Root as Device;
use serde::{Serialize, Deserialize};
use crate::dot_exporter::SiteRoutingGraphDotExporter;
use super::*;

#[derive(Serialize)]
pub struct PinPairRoutingInfo<C> where C: Ord + Eq {
    pub requires: Vec<DNFCube<C>>,
    pub implies: Vec<DNFCube<C>>,
}

impl PinPairRoutingInfo<ConstrainingElement> {
    /* A primitive heuristic for sorting constraints by number of terms.
     * The idea is that a greedy algorithm would set value of the least
     * constraints when placing a cell. Perhaps a better heuristic could
     * be found by performing some stochastic process across all routing
     * infos to try to determine which ones collide with each other the
     * least. */
    fn default_sort(&mut self) {
        let heuristic = |cube: &DNFCube<ConstrainingElement>| cube.len();

        self.implies.sort_by_key(&heuristic);
        self.requires.sort_by_key(&heuristic)
    }
}

impl From<PTPRMarker> for PinPairRoutingInfo<ConstrainingElement> {
    fn from(marker: PTPRMarker) -> Self {
        let mut me = Self {
            requires: marker.constraints.cubes,
            implies: marker.activated.cubes,
        };
        me.default_sort();
        me
    }
}

pub struct RoutingInfo<P> {
    pub pin_to_pin_routing: HashMap<(usize, usize), P>,
    pub out_of_site_sources: HashMap<usize, Vec<usize>>,
    pub out_of_site_sinks: HashMap<usize, Vec<usize>>,
}

pub type RoutingGraphEdge = bool;

#[derive(Clone)]
pub struct RoutingGraphNode {
    pub kind: RoutingGraphNodeKind,
    pub dir: PinDir,
}

#[derive(Clone)]
pub enum RoutingGraphNodeKind {
    BelPort(usize),
    RoutingBelPort(usize),
    SitePort(usize),
    FreePort, /* INVALID */
}

impl Default for RoutingGraphNode {
    fn default() -> Self {
        Self {
            kind: RoutingGraphNodeKind::FreePort,
            dir: PinDir::Inout,
        }
    }
}

pub struct RoutingGraph {
    nodes: Vec<RoutingGraphNode>,
    edges: Vec<RoutingGraphEdge>,  /* Edges between BEL pins */
}

impl RoutingGraph {
    pub fn new(pin_count: usize) -> Self {
        Self {
            nodes: vec![Default::default(); pin_count],
            edges: vec![Default::default(); pin_count * pin_count],
        }
    }

    #[allow(unused)]
    pub fn get_edge<'a>(&'a self, from: usize, to: usize) -> &'a RoutingGraphEdge {
        &self.edges[from * self.nodes.len() + to]
    }

    fn get_edge_mut<'a>(&'a mut self, from: usize, to: usize) -> &'a mut RoutingGraphEdge {
        &mut self.edges[from * self.nodes.len() + to]
    }

    pub fn connect<'a>(&'a mut self, from: usize, to: usize)
        -> Option<&'a mut RoutingGraphEdge>
    {
        let edge = self.get_edge_mut(from, to);

        match edge {
            true => None,
            false => {
                *edge = true;
                Some(edge)
            }
        }
    }

    #[allow(unused)]
    pub fn get_node<'a>(&'a self, node: usize) -> &'a RoutingGraphNode {
        &self.nodes[node]
    }

    fn get_node_mut<'a>(&'a mut self, node: usize) -> &'a mut RoutingGraphNode {
        &mut self.nodes[node]
    }

    pub fn edges_from<'a>(&'a self, from: usize) -> impl Iterator<Item = usize> + 'a {
        self.edges.iter()
            .skip(from * self.nodes.len())
            .take(self.nodes.len())
            .enumerate()
            .filter(|(_, e)| **e)
            .map(|(idx, _)| idx)
    }

    pub fn edges_to<'a>(&'a self, to: usize) -> impl Iterator<Item = usize> + 'a {
        self.edges.iter()
            .skip(to)
            .step_by(self.nodes.len())
            .take(self.nodes.len())
            .enumerate()
            .filter(|(_, e)| **e)
            .map(|(idx, _)| idx)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

/* This enum is currently being reused for both constraint requirements
 * and constraint activators, but later it it might prove to be useful to 
 * have two different enums for activators and requirements. */
#[derive(PartialOrd, PartialEq, Ord, Eq, Clone, Debug, Serialize, Deserialize)]
pub enum ConstrainingElement {
    Port(u32),
}

#[derive(Debug)]
pub struct PortToPortRouterFrame<A> {
    pub prev_node: Option<TilePinId>,
    pub node: TilePinId,
    pub accumulator: A
}

struct PortToPortRouter<'g, A> where A: Default + Clone + std::fmt::Debug + 'static {
    graph: &'g RoutingGraph,
    from: TilePinId,
    markers: Vec<PTPRMarker>,
    queue: VecDeque<PortToPortRouterFrame<A>>,
    callback: &'g Option<BruteRouterCallback<A>>,
}

#[derive(Serialize, Deserialize)]
struct PTPRMarker {
    constraints: DNFForm<ConstrainingElement>,
    activated: DNFForm<ConstrainingElement>,
}

impl<'g, A> PortToPortRouter<'g, A> where A: Default + Clone + std::fmt::Debug + 'static {
    fn new(
        graph: &'g RoutingGraph,
        from: TilePinId,
        callback: &'g Option<BruteRouterCallback<A>>
    ) -> Self {
        Self {
            graph,
            from,
            markers: (0 .. graph.nodes.len()).map(|_| {
                PTPRMarker {
                    constraints: DNFForm::new(),
                    activated: DNFForm::new(),
                }
            }).collect(),
            queue: VecDeque::new(),
            callback,
        }
    }
    
    fn routing_step(&mut self) -> Option<TilePinId> {
        let frame = self.queue.pop_front()?;

        dbg_log!(DBG_EXTRA2, "(RS FRAME) {:?}", frame);

        /* Callbacks for debugging */
        let (mut add_creq_cb, mut add_cact_cb, new_acc) =
            self.callback.as_ref().map(|callback| {
                let mut cb = callback.deref().lock().unwrap();
                cb(&frame)
            }).unwrap_or((None, None, frame.accumulator));

        self.scan_and_add_constraint_requirements(frame.node, frame.prev_node);
        self.scan_and_add_constraint_activators(frame.node, frame.prev_node);
        
        for next in self.graph.edges_from(frame.node.0) {
            let is_subformular = {
                let my_constr = &self.markers[frame.node.0].constraints;
                let next_constr = &self.markers[next].constraints;
                my_constr.is_subformula_of(next_constr)
            };
            
            if !is_subformular {
                let my_constr = self.markers[frame.node.0].constraints.clone();
                let my_activated = self.markers[frame.node.0].activated.clone();

                /* Callbacks for debugging */
                add_creq_cb.as_mut().map(|cb| cb(my_constr.clone()));
                add_cact_cb.as_mut().map(|cb| cb(my_activated.clone()));

                replace_with_or_abort(&mut self.markers[next].constraints, |c| {
                    c.disjunct(my_constr)
                });
                replace_with_or_abort(&mut self.markers[next].activated, |c| {
                    c.disjunct(my_activated)
                });
                self.queue.push_back(PortToPortRouterFrame {
                    prev_node: Some(frame.node),
                    node: TilePinId(next),
                    accumulator: new_acc.clone(),
                });
            }
        }

        Some(frame.node)
    }

    fn scan_and_add_constraint_requirements(
        &mut self,
        node: TilePinId, 
        previous: Option<TilePinId>
    ) {
        if let Some(prev) = previous {
            /* Add constraints for no multiple drivers */
            for driver in self.graph.edges_to(node.0) {
                if driver == prev.0 { continue; }
                replace_with_or_abort(&mut self.markers[node.0].constraints, |c| {
                    /* XXX: Last cube must've been added by us, so we won't modify
                     * constraints for different routes. */
                    c.conjunct_term_with_last(
                        FormulaTerm::NegVar(ConstrainingElement::Port(driver as u32))
                    )
                });
            }
        }

        dbg_log!(
            DBG_EXTRA1,
            "Added constraints for node {}. Current status: {:?}",
            node.0,
            self.markers[node.0].constraints
        );
    }

    fn scan_and_add_constraint_activators(
        &mut self,
        node: TilePinId, 
        previous: Option<TilePinId>
    ) {
        if let Some(previous) = previous  {
            let mut must_activate_driver = false;
            for pnode in self.graph.edges_to(node.0) {
                if pnode != previous.0 {
                    must_activate_driver = true;
                    break;
                }
            }
            if must_activate_driver {
                replace_with_or_abort(&mut self.markers[node.0].activated, |a|
                    a.conjunct_term_with_last(
                        FormulaTerm::Var(ConstrainingElement::Port(previous.0 as u32))
                    )
                );
            }
        }
    }

    fn init_constraints_and_activators(&mut self, node: usize) {
        self.markers[node].constraints = DNFForm::new()
            .add_cube(DNFCube { terms: vec![FormulaTerm::True] });
        self.markers[node].activated = DNFForm::new()
            .add_cube(DNFCube::new());
    }

    fn route_all(mut self, mut step_counter: Option<&mut usize>) -> Vec<PTPRMarker> {
        #[cfg(debug_assertions)]
        if let Some(ref mut counter) = step_counter { **counter = 0 };

        self.init_constraints_and_activators(self.from.0);

        self.queue.clear();
        self.queue.push_back(PortToPortRouterFrame {
            prev_node: None,
            node: self.from,
            accumulator: Default::default(),
        });
        loop {
            #[cfg(debug_assertions)]
            if let Some(ref mut counter) = step_counter { **counter += 1 };

            if let None = self.routing_step() { return self.markers; }
        }
    }
}

pub type BruteRouterCallback<A> = 
    Arc<Mutex<Box<dyn FnMut(&PortToPortRouterFrame<A>)
        -> (
            Option<Box<dyn FnMut(DNFForm<ConstrainingElement>) + Send + 'static>>,
            Option<Box<dyn FnMut(DNFForm<ConstrainingElement>) + Send + 'static>>,
            A
        ) + Send
    >>>;

pub struct BruteRouter<A> {
    tt_id: u32,
    bels: Vec<BELInfo>,
    tile_belpin_idx_to_bel_pin: Vec<(usize, usize)>,
    graph: RoutingGraph,
    callback: Option<BruteRouterCallback<A>>,
}

impl<A> BruteRouter<A> where A: Default + Clone + std::fmt::Debug + 'static {
    pub fn new<'a>(device: &'a Device<'a>, tt_id: u32) -> Self {
        let tt = device.get_tile_type_list().unwrap().get(tt_id);

        /* Create mappings between elements and indices */
        let bels = gather_bels_in_tile_type(&device, &tt);

        let mut bel_name_to_bel_idx = HashMap::new();
        let mut tile_belpin_idx = HashMap::new();
        let mut tile_belpin_idx_to_bel_pin = Vec::new();

        let mut belpin_idx = 0;
        for (bel_idx, bel) in bels.iter().enumerate() {
            let id = (bel.site_type_idx, bel.name);
            if let Some(other_idx) = bel_name_to_bel_idx.insert(id, bel_idx) {
                let name = device.ic_str(bel.name).unwrap();
                let st_list = tt.reborrow().get_site_types().unwrap();
                let other_st = device.reborrow().get_site_type_list().unwrap()
                    .get(st_list.get(bels[other_idx].site_type_idx).get_primary_type());
                let st = device.reborrow().get_site_type_list().unwrap()
                    .get(st_list.get(bel.site_type_idx).get_primary_type());
                
                panic!(
                    concat!(
                        "Conflicting BELs in tile type {}! ({}) {} conflicts with {}. ",
                        "Site types are {} and {}."
                    ),
                    device.ic_str(tt.get_name()).unwrap(),
                    name,
                    bel_idx,
                    other_idx,
                    device.ic_str(other_st.get_name()).unwrap(),
                    device.ic_str(st.get_name()).unwrap(),
                );
            }
            for pin_idx in 0 .. bel.pins.len() {
                let r = tile_belpin_idx.insert((bel_idx, pin_idx), belpin_idx);
                assert!(r.is_none());
                tile_belpin_idx_to_bel_pin.push((bel_idx, pin_idx));
                belpin_idx += 1;
            }
        }

        let graph = Self::create_routing_graph(
            device,
            &tt,
            &bels,
            &bel_name_to_bel_idx,
            &tile_belpin_idx
        );

        assert_eq!(tile_belpin_idx_to_bel_pin.len(), graph.nodes.len());

        Self {
            tt_id,
            bels,
            tile_belpin_idx_to_bel_pin,
            graph,
            callback: None,
        }
    }

    pub fn with_callback<F>(self, callback: F) -> Self where 
        F: FnMut(&PortToPortRouterFrame<A>)
            -> (
                Option<Box<dyn FnMut(DNFForm<ConstrainingElement>) + Send>>,
                Option<Box<dyn FnMut(DNFForm<ConstrainingElement>) + Send>>,
                A
            ) + Send + 'static
    {
        Self {
            callback: Some(Arc::new(Mutex::new(Box::new(callback)))),
            .. self
        }
    }

    fn create_routing_graph<'a>(
        device: &'a Device<'a>,
        tt: &crate::ic_loader::archdef::TileTypeReader<'a>,
        bels: &[BELInfo],
        bel_name_to_bel_idx: &HashMap<(u32, u32), usize>,
        tile_belpin_idx: &HashMap<(usize, usize), usize>)
    -> RoutingGraph
    {
        /* Create routing graph: conections between BELs */
        let mut graph = RoutingGraph::new(tile_belpin_idx.len());
        for (stitt_idx, stitt) in tt.get_site_types().unwrap().iter().enumerate() {
            let site_type_idx = stitt.get_primary_type();
            let site_type = device.reborrow()
                .get_site_type_list().unwrap()
                .get(site_type_idx);
            
            /* Initialize BELs associated with nodes */
            for (bel_idx, bel) in bels.iter().enumerate() {
                for (pin_in_bel_idx, _) in bel.pins.iter().enumerate() {
                    let pin_idx = tile_belpin_idx[&(bel_idx, pin_in_bel_idx)];
                    graph.get_node_mut(pin_idx).dir = bel.pins[pin_in_bel_idx].dir;
                    
                    match bel.category {
                        /* XXX: We will "upgrade" some of the bel ports to routing bel
                         * ports later */
                        BELCategory::LogicOrRouting =>
                            graph.get_node_mut(pin_idx).kind =
                                RoutingGraphNodeKind::BelPort(bel_idx),
                        BELCategory::SitePort =>
                            graph.get_node_mut(pin_idx).kind =
                                RoutingGraphNodeKind::SitePort(bel_idx),
                    }
                }
            }
            
            for wire in site_type.reborrow().get_site_wires().unwrap() {
                let mut drivers = Vec::new();
                let mut sinks = Vec::new();

                for pin_idx in wire.get_pins().unwrap() {
                    let ic_pin = site_type.reborrow().get_bel_pins().unwrap().get(pin_idx);
                    let bel_idx = bel_name_to_bel_idx[&(stitt_idx as u32, ic_pin.get_bel())];
                    let bel = &bels[bel_idx];
                    let ic_pin_name = ic_pin.get_name();
                    let (pin_idx, pin) = bel.pins.iter()
                        .enumerate()
                        .find(|(_, pin)| pin.name == ic_pin_name).unwrap();    
                    let tbpidx = tile_belpin_idx[&(bel_idx, pin_idx)];
                    if let PinDir::Output | PinDir::Inout = pin.dir {
                        drivers.push(tbpidx);
                    }
                    if let PinDir::Input | PinDir::Inout = pin.dir {
                        sinks.push(tbpidx);
                    }
                }

                for driver in drivers {
                    for sink in &sinks {
                        /* XXX: driver can equal to sink in case of Inout */
                        if driver != *sink {
                            let _ = graph.connect(driver, *sink);
                        }
                    }
                }
            }
        }

        /* Create routing graph: add edges for pseudo-pips (routing BELs) */
        for (stitt_idx, stitt) in tt.get_site_types().unwrap().iter().enumerate() {
            let st_idx = stitt.get_primary_type();
            let st = device.reborrow().get_site_type_list().unwrap().get(st_idx);
            let ic_bel_pins = st.reborrow().get_bel_pins().unwrap();
            for spip in st.get_site_p_i_ps().unwrap() {
                let in_pin_idx = spip.get_inpin();
                let out_pin_idx = spip.get_outpin();
                
                let in_bel_name = ic_bel_pins.get(in_pin_idx).get_bel();
                let bel_in_pin_name = ic_bel_pins.get(in_pin_idx).get_name();
                let out_bel_name = ic_bel_pins.get(out_pin_idx).get_bel();
                let bel_out_pin_name = ic_bel_pins.get(out_pin_idx).get_name();
                /* (Pseudo)PIP should represent a single routing BEL */
                assert!(in_bel_name == out_bel_name);
                
                let bel_idx = bel_name_to_bel_idx[&(stitt_idx as u32, in_bel_name)];
                let bel_in_pin_idx = bels[bel_idx].pins.iter()
                    .enumerate()
                    .find(|(_, pin)| pin.name == bel_in_pin_name)
                    .unwrap().0;
                let bel_out_pin_idx = bels[bel_idx].pins.iter()
                    .enumerate()
                    .find(|(_, pin)| pin.name == bel_out_pin_name)
                    .unwrap().0;
                let tile_in_pin_idx = tile_belpin_idx[&(bel_idx, bel_in_pin_idx)];
                let tile_out_pin_idx = tile_belpin_idx[&(bel_idx, bel_out_pin_idx)];
                
                match graph.get_node_mut(tile_in_pin_idx).kind {
                    ref mut kind @ RoutingGraphNodeKind::BelPort(_) => {
                        *kind = RoutingGraphNodeKind::RoutingBelPort(bel_idx)
                    },
                    RoutingGraphNodeKind::RoutingBelPort(node_bel_idx) =>
                        assert_eq!(node_bel_idx, bel_idx),
                    RoutingGraphNodeKind::SitePort(_) => 
                        panic!("Site PIP includes site port {}", tile_in_pin_idx),
                    RoutingGraphNodeKind::FreePort =>
                        panic!("Pin {} uninitialized", tile_in_pin_idx)
                }
                match graph.get_node_mut(tile_out_pin_idx).kind {
                    ref mut kind @ RoutingGraphNodeKind::BelPort(_) =>
                        *kind = RoutingGraphNodeKind::RoutingBelPort(bel_idx),
                    RoutingGraphNodeKind::RoutingBelPort(node_bel_idx) =>
                        assert_eq!(node_bel_idx, bel_idx),
                    RoutingGraphNodeKind::SitePort(_) => 
                        panic!("Site PIP includes site port {}", tile_in_pin_idx),
                    RoutingGraphNodeKind::FreePort =>
                        panic!("Pin {} uninitialized", tile_in_pin_idx)
                }

                let _ = graph.connect(tile_in_pin_idx, tile_out_pin_idx);
            }
        }

        /* Check that all nodes have been initialized. */
        #[cfg(debug_assertions)]
        assert!(
            if let None = graph.nodes.iter().find(|n| {
                match n.kind {
                    RoutingGraphNodeKind::FreePort => true,
                    _ => false,
                }
            }) { true } else { false }
        );

        graph
    }

    pub fn get_pin_id<'d>(
        &self,
        device: &Device<'d>,
        site_name: &str,
        bel_name: &str,
        pin_name: &str
    )
        -> Result<TilePinId, String>
    {
        let tt = device.get_tile_type_list().unwrap().get(self.tt_id);
        
        let st_list = device.get_site_type_list().unwrap();

        let (stitt_id, _, st_instance_name) = tt.get_site_types().unwrap().iter()
            .enumerate()
            .find_map(|(stitt_id, stitt)| {
                let st = st_list.get(stitt.get_primary_type());
                let st_instance_name =
                    format!("{}_{}", device.ic_str(st.get_name()).unwrap(), stitt_id);
                if st_instance_name == site_name {
                    return Some((stitt_id, stitt, st_instance_name));
                }
                None
            })
            .ok_or_else(|| format!(
                "Site {}/{} not found",
                device.ic_str(tt.get_name()).unwrap(),
                site_name)
            )?;
        
        let mut bel_found = None;
        (0 .. self.graph.nodes.len())
            .find(|belpin_id| {
                let (bel_id, bel_pin_id) = self.tile_belpin_idx_to_bel_pin[*belpin_id];
                if device.ic_str(self.bels[bel_id].name).unwrap() == bel_name {
                    bel_found = Some(bel_id);
                } else {
                    return false;
                } 
                let c_pin_name = self.bels[bel_id].pins[bel_pin_id].name;
                if self.bels[bel_id].site_type_idx != stitt_id as u32 { return false; }
                if device.ic_str(c_pin_name).unwrap() != pin_name { return false; }
                true
            }).ok_or_else(|| match bel_found {
                    Some(bel_id) => format!(
                        "Pin {}/{}/{}.{} not found",
                        device.ic_str(tt.get_name()).unwrap(),
                        st_instance_name,
                        device.ic_str(self.bels[bel_id].name).unwrap(),
                        pin_name
                    ),
                    None => format!(
                        "BEL {}/{}/{} not found",
                        device.ic_str(tt.get_name()).unwrap(),
                        st_instance_name, 
                        bel_name
                    ),
                }
            )
            .map(TilePinId)
    }

    pub fn get_pin_name<'d>(&self, device: &Device<'d>, pin_id: TilePinId)
        -> TilePinName<'static, 'd, 'd, String, &'d str, &'d str>
    {
        let tt = device.get_tile_type_list().unwrap().get(self.tt_id);
        let st_list = device.get_site_type_list().unwrap();
        let stitt_list = tt.get_site_types().unwrap();

        let (bel_id, bel_pin_id) = self.tile_belpin_idx_to_bel_pin[pin_id.0];
        let bel = device.ic_str(self.bels[bel_id].name).unwrap();
        let pin = device.ic_str(self.bels[bel_id].pins[bel_pin_id].name).unwrap();
        let stitt_id = self.bels[bel_id].site_type_idx;
        let stitt = stitt_list.get(stitt_id);
        let st = st_list.get(stitt.get_primary_type());

        let site_instance = format!(
            "{}_{}",
            device.ic_str(st.get_name()).unwrap(),
            stitt_id
        );

        return TilePinName::new(site_instance, bel, pin)
    }

    pub fn route_pins(
        &self,
        from: TilePinId,
        step_counter: Option<&mut usize>, /* Can be used only in debug build */
        optimize: bool
    )
        -> impl Iterator<Item = PinPairRoutingInfo<ConstrainingElement>>
    {
        let router = PortToPortRouter::<A>::new(&self.graph, from, &self.callback);
        router.route_all(step_counter)
            .into_iter()
            .map(move |mut marker| {
                if optimize {
                    marker.constraints = marker.constraints.optimize()
                }
                marker
            })
            .map(Into::into)
    }

    fn route_range(&self, range: std::ops::Range<usize>, optimize: bool)
        -> HashMap<(usize, usize), PinPairRoutingInfo<ConstrainingElement>>
    {
        let pin_cnt = self.graph.nodes.len();
        debug_assert!(range.start < range.end);
        debug_assert!(range.start <= pin_cnt);
        debug_assert!(range.end <= range.end);

        let mut pin_to_pin_map = HashMap::new();
        for from in range {
            if let PinDir::Input = self.graph.get_node(from).dir {
                continue; /* We don't need routing information for input pins */
            }
            dbg_log!(DBG_INFO, "Routing from pin {}/{}", from, pin_cnt);
            let mut step_counter = 0;
            let routing_results =
                self.route_pins(TilePinId(from), Some(&mut step_counter), optimize);
            dbg_log!(DBG_INFO, "  Number of steps: {}", step_counter);
            for (to, routing_info) in routing_results.enumerate() {
                if (routing_info.requires.len() != 0) || (routing_info.implies.len() != 0) {
                    pin_to_pin_map.insert((from, to), routing_info);
                }
            }
        }
        pin_to_pin_map
    }

    fn gather_out_of_site_info(
        &self,
        map: &HashMap<(usize, usize), PinPairRoutingInfo<ConstrainingElement>>
    )
        -> (HashMap<usize, Vec<usize>>, HashMap<usize, Vec<usize>>)
    {
        let mut out_of_site_sources = HashMap::new();
        let mut out_of_site_sinks = HashMap::new();

        for ((from, to), _) in map {
            let from_node = self.graph.get_node(*from);
            if let PinDir::Output | PinDir::Inout = from_node.dir {
                if let RoutingGraphNodeKind::SitePort(_) = from_node.kind {
                    out_of_site_sources.entry(*to).or_insert_with(Vec::new).push(*from);
                }
            }

            let to_node = self.graph.get_node(*to);
            if let PinDir::Input | PinDir::Inout = to_node.dir {
                if let RoutingGraphNodeKind::SitePort(_) = to_node.kind {
                    out_of_site_sinks.entry(*from).or_insert_with(Vec::new).push(*to);
                }
            }
        }

        (out_of_site_sources, out_of_site_sinks)
    }

    pub fn route_all(&self, optimize: bool)
        -> RoutingInfo<PinPairRoutingInfo<ConstrainingElement>>
    {
        let map = self.route_range(
            0 .. self.tile_belpin_idx_to_bel_pin.len(),
            optimize
        );

        let (out_of_site_sources, out_of_site_sinks) =
            self.gather_out_of_site_info(&map);

        RoutingInfo {
            pin_to_pin_routing: map,
            out_of_site_sources,
            out_of_site_sinks,
        }
    }

    pub fn create_dot_exporter<'s>(&'s self, device: &Device<'s>)
        -> SiteRoutingGraphDotExporter<
            &'s RoutingGraph,
            &'s Vec<BELInfo>,
            &'s Vec<(usize, usize)>,
            crate::ic_loader::archdef::TileTypeReader<'s>
           >
        {
        SiteRoutingGraphDotExporter::new(
            &self.graph,
            &self.bels,
            &self.tile_belpin_idx_to_bel_pin,
            device.get_tile_type_list().unwrap().get(self.tt_id)
        )
    }
}

pub trait MultiThreadedBruteRouter<A> {
    fn route_all_multithreaded(self, thread_count: usize, optimize: bool)
        -> RoutingInfo<PinPairRoutingInfo<ConstrainingElement>>;
}

impl<R, A> MultiThreadedBruteRouter<A> for R
where
    R: Borrow<BruteRouter<A>> + Clone + Send + 'static,
    A: Default + Clone + std::fmt::Debug + 'static {
    /* Not the best multithreading, but should improve the runtime nevertheless. */
    fn route_all_multithreaded(self, thread_count: usize, optimize: bool)
        -> RoutingInfo<PinPairRoutingInfo<ConstrainingElement>>
    {
        let mut total_map = HashMap::new();
        let mut handles = Vec::new();
        
        let pin_cnt = self.borrow().tile_belpin_idx_to_bel_pin.len();

        for range in split_range_nicely(0 .. pin_cnt, thread_count) {
            let me = self.clone();
            let handle = thread::spawn(move || {
                me.borrow().route_range(range, optimize)
            });
            handles.push(handle);
        }
        for handle in handles {
            let map = handle.join().unwrap();
            total_map.extend(map.into_iter());
        }

        let (out_of_site_sources, out_of_site_sinks) =
            self.borrow().gather_out_of_site_info(&total_map);

        RoutingInfo {
            pin_to_pin_routing: total_map,
            out_of_site_sources,
            out_of_site_sinks,
        }
    }
}
