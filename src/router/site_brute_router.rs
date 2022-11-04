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
use crate::log::*;
use crate::ic_loader::archdef::Root as Device;
use serde::{Serialize, Deserialize};
use crate::dot_exporter::SiteRoutingGraphDotExporter;
use super::*;

#[derive(Serialize)]
pub struct PinPairRoutingInfo {
    requires: Vec<DNFCube<ConstrainingElement>>,
    implies: Vec<DNFCube<ConstrainingElement>>,
}

impl PinPairRoutingInfo {
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

impl From<PTPRMarker> for PinPairRoutingInfo {
    fn from(marker: PTPRMarker) -> Self {
        let mut me = Self {
            requires: marker.constraints.cubes,
            implies: marker.activated.cubes,
        };
        me.default_sort();
        me
    }
}

pub struct RoutingInfo {
    pub pin_to_pin_routing: HashMap<(usize, usize), PinPairRoutingInfo>,
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
enum ConstrainingElement {
    Port(u32),
}

struct PortToPortRouter<'g> {
    graph: &'g RoutingGraph,
    from: usize,
    markers: Vec<PTPRMarker>,
    queue: VecDeque<(Option<usize>, usize)>,
}

#[derive(Serialize, Deserialize)]
struct PTPRMarker {
    constraints: DNFForm<ConstrainingElement>,
    activated: DNFForm<ConstrainingElement>,
}

impl<'g> PortToPortRouter<'g> {
    fn new(graph: &'g RoutingGraph, from: usize) -> Self {
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
        }
    }

    fn routing_step(&mut self) -> Option<usize> {
        let (previous_node, current_node) = self.queue.pop_front()?;

        self.scan_and_add_constraint_requirements(current_node, previous_node);
        self.scan_and_add_constraint_activators(current_node, previous_node);
        
        for next in self.graph.edges_from(current_node) {
            let is_subformular = {
                let my_constr = &self.markers[current_node].constraints;
                let next_constr = &self.markers[next].constraints;
                my_constr.is_subformula_of(next_constr)
            };
            
            if !is_subformular {
                let my_constr = self.markers[current_node].constraints.clone();
                let my_activated = self.markers[current_node].activated.clone();
                replace_with_or_abort(&mut self.markers[next].constraints, |c| {
                    c.disjunct(my_constr)
                });
                replace_with_or_abort(&mut self.markers[next].activated, |c| {
                    c.disjunct(my_activated)
                });
                self.queue.push_back((Some(current_node), next));
            }
        }

        Some(current_node)
    }

    fn scan_and_add_constraint_requirements(
        &mut self,
        node: usize, 
        previous: Option<usize>
    ) {
        if let Some(prev) = previous {
            /* Add constraints for no multiple drivers */
            for driver in self.graph.edges_to(node) {
                if driver == prev { continue; }
                replace_with_or_abort(&mut self.markers[node].constraints, |c| {
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
            node,
            self.markers[node].constraints
        );
    }

    fn scan_and_add_constraint_activators(
        &mut self,
        node: usize, 
        previous: Option<usize>
    ) {
        if let Some(previous) = previous  {
            let mut must_activate_driver = false;
            for pnode in self.graph.edges_to(node) {
                if pnode != previous {
                    must_activate_driver = true;
                    break;
                }
            }
            if must_activate_driver {
                replace_with_or_abort(&mut self.markers[node].activated, |a|
                    a.conjunct_term_with_last(
                        FormulaTerm::Var(ConstrainingElement::Port(previous as u32))
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

        self.init_constraints_and_activators(self.from);

        self.queue.clear();
        self.queue.push_back((None, self.from));
        loop {
            #[cfg(debug_assertions)]
            if let Some(ref mut counter) = step_counter { **counter += 1 };

            if let None = self.routing_step() { return self.markers; }
        }
    }
}

pub struct BruteRouter<'a> {
    tt: crate::ic_loader::archdef::TileTypeReader<'a>,
    bels: Vec<BELInfo>,
    tile_belpin_idx_to_bel_pin: Vec<(usize, usize)>,
    graph: RoutingGraph,
}

impl<'a> BruteRouter<'a> {
    pub fn new(
        device: &'a Device<'a>,
        tt: &crate::ic_loader::archdef::TileTypeReader<'a>,
    ) -> Self {

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
            tt,
            &bels,
            &bel_name_to_bel_idx,
            &tile_belpin_idx
        );

        assert_eq!(tile_belpin_idx_to_bel_pin.len(), graph.nodes.len());

        Self {
            tt: tt.clone(),
            bels,
            tile_belpin_idx_to_bel_pin,
            graph,
        }
    }

    fn create_routing_graph(
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

    fn route_pins(
        graph: &RoutingGraph,
        from: usize,
        step_counter: Option<&mut usize>, /* Can be used only in debug build */
        optimize: bool
    ) -> impl Iterator<Item = PinPairRoutingInfo> {
        let router = PortToPortRouter::new(graph, from);
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

    fn route_range(graph: &RoutingGraph, range: std::ops::Range<usize>, optimize: bool)
        -> HashMap<(usize, usize), PinPairRoutingInfo>
    {
        let pin_cnt = graph.nodes.len();
        debug_assert!(range.start < range.end);
        debug_assert!(range.start <= pin_cnt);
        debug_assert!(range.end <= range.end);

        let mut pin_to_pin_map = HashMap::new();
        for from in range {
            if let PinDir::Input = graph.get_node(from).dir {
                continue; /* We don't need routing information for input pins */
            }
            dbg_log!(DBG_INFO, "Routing from pin {}/{}", from, pin_cnt);
            let mut step_counter = 0;
            let routing_results =
                Self::route_pins(graph, from, Some(&mut step_counter), optimize);
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
        graph: &RoutingGraph,
        map: &HashMap<(usize, usize), PinPairRoutingInfo>
    )
        -> (HashMap<usize, Vec<usize>>, HashMap<usize, Vec<usize>>)
    {
        let mut out_of_site_sources = HashMap::new();
        let mut out_of_site_sinks = HashMap::new();

        for ((from, to), _) in map {
            let from_node = graph.get_node(*from);
            if let PinDir::Output | PinDir::Inout = from_node.dir {
                if let RoutingGraphNodeKind::SitePort(_) = from_node.kind {
                    out_of_site_sources.entry(*to).or_insert_with(Vec::new).push(*from);
                }
            }

            let to_node = graph.get_node(*to);
            if let PinDir::Input | PinDir::Inout = to_node.dir {
                if let RoutingGraphNodeKind::SitePort(_) = to_node.kind {
                    out_of_site_sinks.entry(*from).or_insert_with(Vec::new).push(*to);
                }
            }
        }

        (out_of_site_sources, out_of_site_sinks)
    }

    pub fn route_all(self, optimize: bool) -> RoutingInfo {
        let map = Self::route_range(
            &self.graph,
            0 .. self.tile_belpin_idx_to_bel_pin.len(),
            optimize
        );

        let (out_of_site_sources, out_of_site_sinks) =
            Self::gather_out_of_site_info(&self.graph, &map);


        RoutingInfo {
            pin_to_pin_routing: map,
            out_of_site_sources,
            out_of_site_sinks,
        }
    }

    /* Not the best multithreading, but should improve the runtime nevertheless. */
    pub fn route_all_multithreaded(self, thread_count: usize, optimize: bool)
        -> RoutingInfo
    {
        use std::sync::Arc;

        let mut total_map = HashMap::new();
        let mut handles = Vec::new();
        
        let pin_cnt = self.tile_belpin_idx_to_bel_pin.len();

        let graph = Arc::new(self.graph);
        for range in split_range_nicely(0 .. pin_cnt, thread_count) {
            let graph = Arc::clone(&graph);
            let handle = thread::spawn(move || {
                let graph = graph.deref();
                Self::route_range(graph, range, optimize)
            });
            handles.push(handle);
        }
        for handle in handles {
            let map = handle.join().unwrap();
            total_map.extend(map.into_iter());
        }

        let (out_of_site_sources, out_of_site_sinks) =
            Self::gather_out_of_site_info(&*graph, &total_map);

        RoutingInfo {
            pin_to_pin_routing: total_map,
            out_of_site_sources,
            out_of_site_sinks,
        }
    }

    pub fn create_dot_exporter<'s: 'a>(&'s self)
        -> SiteRoutingGraphDotExporter<
            &'s RoutingGraph,
            &'s Vec<BELInfo>,
            &'s Vec<(usize, usize)>,
            &'s crate::ic_loader::archdef::TileTypeReader<'a>
           >
        {
        SiteRoutingGraphDotExporter::new(
            &self.graph,
            &self.bels,
            &self.tile_belpin_idx_to_bel_pin,
            &self.tt
        )
    }
}