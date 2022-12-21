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
pub struct PinPairRoutingInfo {
    pub requires: Vec<DNFCube<ConstrainingElement>>,
    pub implies: Vec<DNFCube<ConstrainingElement>>,
}

impl PinPairRoutingInfo {
    /// A primitive heuristic for sorting constraints by number of terms.
    /// The idea is that a greedy algorithm would set value of the least
    /// constraints when placing a cell. Perhaps a better heuristic could
    /// be found by performing some stochastic process across all routing
    /// infos to try to determine which ones collide with each other the
    /// least.
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
    pub pin_to_pin_routing: HashMap<(SitePinId, SitePinId), PinPairRoutingInfo>,
    pub out_of_site_sources: HashMap<SitePinId, Vec<SitePinId>>,
    pub out_of_site_sinks: HashMap<SitePinId, Vec<SitePinId>>,
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
/// Represents a resource congesting nets.
#[derive(PartialOrd, PartialEq, Ord, Eq, Clone, Debug, Serialize, Deserialize)]
pub enum ConstrainingElement {
    /// Usage of a port
    Port(u32),
}

#[derive(Debug)]
pub struct PortToPortRouterFrame<A> {
    #[cfg(debug_assertions)]
    _step: usize,
    pub prev_node: Option<SitePinId>,
    pub node: SitePinId,
    #[cfg(debug_assertions)]
    _cube_count: usize,
    pub accumulator: A,
}

/// PortToPort Router represents a routing context for net expansion coming from a selected
/// pin. The algorithm aims to expand the net to reach all possible values and gather
/// constraints along the way that would be used later to drive a placer to avoid net
/// congestions.
/// 
/// # The algorithm
/// 
/// The algorithm is based on BFS. a queue is used to store frames containing data relevant
/// to a routing step, most importantly - the node queued, the node which queued it and
/// accumulator.
/// 
/// Each step a frame is being popped from the queue and the algorithm decides whether new
/// constraints for routes coming from the `prev_node` should be added. Similarly, the
/// algorithm decides if those routes should also trigger certains states (activators).
/// 
/// The constraints are represented by DNF formulas. The conjunction blocks (aka. "cubes")
/// represent a set of constraints that need to be met for a sinlge route. The disjunctions
/// between those blocks happen when the node can be entered by more than a single route.
/// 
/// Whether the route should be pursued any further is decided by the algorithm based on
/// these constraints. If the constraints of the current node are less strict than those
/// for the next candidatem the candidate should be queued.
struct PortToPortRouter<'g, A> where A: Default + Clone + std::fmt::Debug + 'static {
    graph: &'g RoutingGraph,
    from: SitePinId,
    markers: Vec<PTPRMarker>,
    queue: VecDeque<PortToPortRouterFrame<A>>,
    callback: &'g Option<BruteRouterCallback<A>>,
    optimize_implies: bool,
}

#[derive(Serialize, Deserialize)]
struct PTPRMarker {
    constraints: DNFForm<ConstrainingElement>,
    activated: DNFForm<ConstrainingElement>,
}

impl<'g, A> PortToPortRouter<'g, A> where A: Default + Clone + std::fmt::Debug + 'static {
    fn new(
        graph: &'g RoutingGraph,
        from: SitePinId,
        callback: &'g Option<BruteRouterCallback<A>>,
        optimize_implies: bool
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
            optimize_implies,
        }
    }

    fn is_constr_subformular(&self, a: Option<SitePinId>, b: SitePinId) -> bool {
        if let Some(SitePinId(a)) = a {
            let my_form = &self.markers[a].constraints;
            let other_form = &self.markers[b.0].constraints;
            my_form.is_subformula_of(other_form)
        } else {
            true
        }
    }

    fn is_activators_subformular(&self, a: Option<SitePinId>, b: SitePinId) -> bool {
        if let Some(SitePinId(a)) = a {
            let my_form = &self.markers[a].activated;
            let other_form = &self.markers[b.0].activated;
            my_form.is_subformula_of(other_form)
        } else {
            true
        }
    }
    
    /// Performs a routing step of the `PortToPortRouter`:
    /// 1. Pop a frame from a queue
    /// 2. Process associated node and update routability information
    /// 3. Queue new frames
    /// 
    /// # Return
    /// The returned value is an ID of the processed node.
    fn routing_step(&mut self) -> Option<SitePinId> {
        let frame = self.queue.pop_front()?;

        dbg_log!(DBG_EXTRA2, "(RS FRAME) {:?}", frame);

        /* Callbacks for debugging */
        let (mut add_creq_cb, mut add_cact_cb, new_acc) =
            self.callback.as_ref().map(|callback| {
                let mut cb = callback.deref().lock().unwrap();
                cb(&frame)
            }).unwrap_or((None, None, frame.accumulator));
        
        /* I hoped that Cow could be used to avoid making needless copies each iteration,
         * but Cow has the dumbest ToOwned implmentation (blanket implmentation) */
        let new_requirements = match frame.prev_node {
            Some(prev) => self.markers[prev.0].constraints.clone(),
            None => DNFForm::new().add_cube(DNFCube::new()),
        };
        let new_requirements =
            self.scan_constraint_requirements(frame.node, frame.prev_node)
                .fold(new_requirements, |r, c| r.conjunct_term(&c));
        
        add_creq_cb.as_mut().map(|cb| cb(new_requirements.clone()));
        
        let needs_alternative = !self.is_constr_subformular(frame.prev_node, frame.node);
        replace_with_or_abort(&mut self.markers[frame.node.0].constraints, |c| {
            if needs_alternative {
                c.disjunct(new_requirements)
            } else {
                new_requirements
            }
        });

        let new_activators = match frame.prev_node {
            Some(prev) => self.markers[prev.0].activated.clone(),
            None => DNFForm::new().add_cube(DNFCube::new()),
        };
        let new_activators =
            self.scan_constraint_activators(frame.node, frame.prev_node)
                .fold(new_activators, |r, c| r.conjunct_term(&c));

        add_cact_cb.as_mut().map(|cb| cb(new_activators.clone()));

        let needs_alternative = if self.optimize_implies {
            !self.is_activators_subformular(frame.prev_node, frame.node)
        } else {
            needs_alternative
        };
        replace_with_or_abort(&mut self.markers[frame.node.0].activated, |c| {
            if needs_alternative {
                c.disjunct(new_activators)
            } else {
                new_activators
            }
        });

        dbg_log!(
            DBG_EXTRA2,
            "    constraints: {:?}",
            self.markers[frame.node.0].constraints
        );
        
        for next in self.graph.edges_from(frame.node.0) {
            let is_subformular =
                self.is_constr_subformular(Some(frame.node), SitePinId(next));
            if !is_subformular {
                self.queue.push_back(PortToPortRouterFrame {
                    #[cfg(debug_assertions)]
                    _step: frame._step + 1,
                    prev_node: Some(frame.node),
                    node: SitePinId(next),
                    #[cfg(debug_assertions)]
                    _cube_count: self.markers[frame.node.0].constraints.num_cubes(),
                    accumulator: new_acc.clone(),
                });
            }
        }

        Some(frame.node)
    }

    fn scan_constraint_requirements(&self, node: SitePinId, prev_node: Option<SitePinId>)
        -> impl Iterator<Item = FormulaTerm<ConstrainingElement>> + 'g
    {
        /* Add constraints for no multiple drivers (yield all except prev_node) */
        let graph = self.graph;
        prev_node.into_iter().map(move |prev| {
            graph.edges_to(node.0).filter_map(move |driver| {
                (driver != prev.0)
                    .then(|| FormulaTerm::NegVar(ConstrainingElement::Port(driver as u32)))
            })
        }).flatten()
    }

    fn scan_constraint_activators(&self, node: SitePinId, prev_node: Option<SitePinId>)
        -> impl Iterator<Item = FormulaTerm<ConstrainingElement>> + 'g
    {
        let graph = self.graph;
        prev_node.into_iter().map(move |prev| {
            graph.edges_to(node.0).filter_map(move |pnode| {
                (pnode == prev.0)
                    .then(|| FormulaTerm::Var(ConstrainingElement::Port(prev.0 as u32)))  
            })
        }).flatten()
    }

    fn init_constraints_and_activators(&mut self, node: usize) {
        self.markers[node].constraints = DNFForm::new()
            .add_cube(DNFCube::new());
        self.markers[node].activated = DNFForm::new()
            .add_cube(DNFCube::new());
    }

    fn route_all(mut self) -> Vec<PTPRMarker> {
        self.init_constraints_and_activators(self.from.0);

        self.queue.clear();
        self.queue.push_back(PortToPortRouterFrame {
            #[cfg(debug_assertions)]
            _step: 0,
            prev_node: None,
            node: self.from,
            #[cfg(debug_assertions)]
            _cube_count: 1,
            accumulator: Default::default(),
        });
        loop {
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
    st_id: u32,
    bels: Vec<BELInfo>,
    site_belpin_idx_to_bel_pin: Vec<(usize, usize)>,
    graph: RoutingGraph,
    callback: Option<BruteRouterCallback<A>>,
}

impl<A> BruteRouter<A> where A: Default + Clone + std::fmt::Debug + 'static {
    /// Create new BruteRouter for tile wit ID `tt_id`.
    /// 
    /// The `add_virtual_consts` parameter extends the sites in the tile with BELs which are
    /// used by nextpnr/python-fpga-interchange to model constant nets.
    /// 
    /// The approach can be summarized uing the following graph:
    /// ```text
    ///         ╔════════════════════════════════════════════════════════════╗
    ///         ║                            SITE                            ║
    ///     A━━━║────────────────────────────────────────────┐               ║
    ///     B━━━║───────────────────────────┐                │               ║
    ///     C━━━║──────────┐ ┏━━━━━━━━━┓    │ ┏━━━━━━━━━┓    │ ┏━━━━━━━━━┓   ║
    ///         ║          └─┃  OTHER  ┃    └─┃ ANOTHER ┃    └─┃   YET   ┃   ║
    ///         ║            ┃         ┃      ┃         ┃      ┃ ANOTHER ┃   ║
    ///         ║          ┌─┃   BEL   ┃─┐  ┌─┃   BEL   ┃─┐  ┌─┃   BEL   ┃─┐ ║
    ///         ║          │ ┗━━━━━━━━━┛ │  │ ┗━━━━━━━━━┛ │  │ ┗━━━━━━━━━┛ │ ║
    ///         ║          │             └──│─────────────│──│─────────────│─║━━━X
    ///         ║          │                │             └──│─────────────│─║━━━Y
    ///         ║ ┏━━━━━┓  │       ┏━━━━━┓  │       ┏━━━━━┓  │             └─║━━━Z
    ///         ║ ┃ VCC ┃  │       ┃ VCC ┃  │       ┃ GND ┃  │               ║
    ///         ║ ┃ GEN ┃─┬┘       ┃ GEN ┃─┬┘       ┃ GEN ┃─┬┘               ║
    ///         ║ ┗━━━━━┛ │        ┗━━━━━┛ │        ┗━━━━━┛ │                ║
    ///         ║         ◦                ◦                ◦                ║
    ///         ║         ┊                ┊                ┊                ║
    ///  $VCC┉┉┉║┈┈┈┈┈┈┈┈┈┴┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┘                ┊                ║
    ///  $GND┉┉┉║┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┘                ║
    ///         ╚════════════════════════════════════════════════════════════╝
    /// ```
    /// 
    /// If a site has a constant source BEL, an input port will be generated, called either
    /// `$VCC` or `$GND` depending on the constant. The an extra wire, coming from it will be
    /// created and connected via an extra PIP BELs to each uwire coming from a BEL generating
    /// this constant.
    /// 
    /// On the illustration, two constants are generated by three BELs - two generate VCC and
    /// one generates GND. Two extra site inputs called `$VCC` and `$GND` have been added to
    /// the site, each with its own wire (dotted lines). Those wires were then connected to the
    /// already present wires coming from the generators via extra BELs which act as PIPs.
    /// These are marked with `◦` symbol.
    /// 
    /// # Arguments
    /// * `device` - `DeviceResources::Device` root
    /// * `st_id` - SiteType's ID
    /// * `add_virtual_consts` - add extra BELs and connections used for constant networks,
    ///   see the explanations above
    pub fn new<'a>(device: &'a Device<'a>, st_id: u32, add_virtual_consts: bool) -> Self {
        let st = device.get_site_type_list().unwrap().get(st_id);

        /* Create mappings between elements and indices */
        let bels = gather_bels_in_site_type(&device, &st, add_virtual_consts);

        let mut bel_name_to_bel_idx = HashMap::new();
        let mut tile_belpin_idx = HashMap::new();
        let mut tile_belpin_idx_to_bel_pin = Vec::new();

        let mut belpin_idx = 0;
        for (bel_idx, bel) in bels.iter().enumerate() {
            if bel_name_to_bel_idx.insert(bel.name, bel_idx).is_some() {
                let gsctx = GlobalStringsCtx::hold();
                panic!("BEL {} already exists", bel.name.get(device, &gsctx));
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
            &st,
            &bels,
            &bel_name_to_bel_idx,
            &tile_belpin_idx,
            add_virtual_consts
        );

        assert_eq!(tile_belpin_idx_to_bel_pin.len(), graph.nodes.len());

        Self {
            st_id,
            bels,
            site_belpin_idx_to_bel_pin: tile_belpin_idx_to_bel_pin,
            graph,
            callback: None,
        }
    }
    
    /// Add a callback to the siterouter. The callback will be executed at each step
    /// and will gain access to the accumulator used by the router.
    /// The callback should return a new value for the accumulator, that will be
    /// passed to the queued frames of the router. The callback can also return another
    /// callbacks to be called for each new contraining formula or activators formula
    /// added.
    /// 
    /// # Arguments
    /// * `callback` - the callback to be used by the router.
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

    /// Initialize BEL information associated with graph nodes
    fn init_bels_in_graph(
        graph: &mut RoutingGraph,
        bels: &[BELInfo],
        tile_belpin_idx: &HashMap<(usize, usize), usize>
    ) {
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
    }

    /// Create connections between BELs in site's routing graph based on the site wires
    /// present in DeviceResources.
    fn init_site_wires_in_graph<'d>(
        graph: &mut RoutingGraph,
        st: &crate::ic_loader::archdef::SiteTypeReader<'d>,
        bels: &[BELInfo],
        bel_name_to_bel_idx: &HashMap<ResourceName, usize>,
        tile_belpin_idx: &HashMap<(usize, usize), usize>
    ) {
        let sw_list = st.get_site_wires().unwrap();
        
        for wire in sw_list {
            let mut drivers = Vec::new();
            let mut sinks = Vec::new();

            for pin_idx in wire.get_pins().unwrap() {
                let ic_pin = st.reborrow().get_bel_pins().unwrap().get(pin_idx);
                let bel_idx = bel_name_to_bel_idx[
                    &ResourceName::DeviceResources(ic_pin.get_bel())
                ];
                let bel = &bels[bel_idx];
                let ic_pin_name = ic_pin.get_name();
                let (pin_idx, pin) = bel.pins.iter()
                    .enumerate()
                    .find(|(_, pin)|
                        pin.name == ResourceName::DeviceResources(ic_pin_name)
                    ).unwrap();    
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

    /// Create connections that represent pseudo-PIPs (routing BELs) in site's routing graph.
    fn init_pseudopips_in_graph<'d>(
        graph: &mut RoutingGraph,
        st: &crate::ic_loader::archdef::SiteTypeReader<'d>,
        bels: &[BELInfo],
        bel_name_to_bel_idx: &HashMap<ResourceName, usize>,
        tile_belpin_idx: &HashMap<(usize, usize), usize>
    ) {
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
            
            let bel_idx = bel_name_to_bel_idx[
                &ResourceName::DeviceResources(in_bel_name)
            ];
            let bel_in_pin_idx = bels[bel_idx].pins.iter()
                .enumerate()
                .find(|(_, pin)|
                    pin.name == ResourceName::DeviceResources(bel_in_pin_name)
                )
                .unwrap().0;
            let bel_out_pin_idx = bels[bel_idx].pins.iter()
                .enumerate()
                .find(|(_, pin)|
                    pin.name == ResourceName::DeviceResources(bel_out_pin_name)
                ).unwrap().0;
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

    /// Creates connections between `$VCC`, `$GND` site-ports and routing BELs associated
    /// with virtual constant nets
    fn init_virtual_wires_in_graph<'d>(
        graph: &mut RoutingGraph,
        device: &Device<'d>,
        st: &crate::ic_loader::archdef::SiteTypeReader<'d>,
        bels: &[BELInfo],
        bel_name_to_bel_idx: &HashMap<ResourceName, usize>,
        tile_belpin_idx: &HashMap<(usize, usize), usize>
    ) {
        use crate::ic_loader::DeviceResources_capnp::device::ConstantType;
        
        let sw_list = st.get_site_wires().unwrap();
    
        let mut gsctx = GlobalStringsCtx::hold();

        let vcc_bel_name = gsctx.create_global_string("$VCC");
        let vcc_bel_idx_opt = bel_name_to_bel_idx.get(
            &ResourceName::Virtual(vcc_bel_name)
        );
        
        let gnd_bel_name = gsctx.create_global_string("$GND");
        let gnd_bel_idx_opt = bel_name_to_bel_idx.get(
            &ResourceName::Virtual(gnd_bel_name)
        );

        let bel_pin_to_wire = create_belname_pinname_to_wire_lookup(&st);

        let sources =
            device.get_constants().unwrap().get_site_sources().unwrap().iter()
                .filter(|src| src.get_site_type() == st.get_name());
        
        let mut gsctx = GlobalStringsCtx::hold();

        for src in sources {
            let src_bel_name = device.ic_str(src.get_bel());

            let (from_bel_idx, from_bel_name) = match src.get_constant().unwrap() {
                ConstantType::Vcc => match vcc_bel_idx_opt {
                    Some(bel_idx) => (bel_idx, vcc_bel_name),
                    None => panic!("VCC site source found but no $VCC port was added."),
                },
                ConstantType::Gnd => match gnd_bel_idx_opt {
                    Some(bel_idx) => (bel_idx, gnd_bel_name),
                    None => panic!("GND site source found but no $GND port was added."),
                },
                _ => panic!("Usupported constant type"),
            };

            /* XXX: This is a site-pin BEL. It has only one pin. */
            let from_belpin_idx = *tile_belpin_idx.get(&(*from_bel_idx, 0))
                .unwrap();

            let src_bel_idx = bel_name_to_bel_idx[
                &ResourceName::DeviceResources(src.get_bel())
            ];

            let src_pin_idx = bels[src_bel_idx].find_pin(
                ResourceName::DeviceResources(src.get_bel_pin())
            ).unwrap();

            let src_belpin = tile_belpin_idx[&(src_bel_idx, src_pin_idx)];
            let src_wire_id = bel_pin_to_wire[&(src.get_bel(), src.get_bel_pin())];
            let src_wire = sw_list.get(src_wire_id);

            /* Borrowing rules won't let me iterate over connections and add
                * new ones at the same time.
                * TODO: implement an associated function for `RoutingGraph` that would
                * allow creating new edges while iterating over existing ones. */
            let sinks: Vec<_> = graph.edges_from(src_belpin).collect();

            let net_name = gsctx.get_global_string(from_bel_name).to_string();
            let pip_bel_name =
                create_vconst_net_pipbel_name(src_bel_name, &net_name, &mut gsctx);
            
            let pip_bel_idx = bel_name_to_bel_idx[&pip_bel_name];

            let pip_bel_input_pin_idx = bels[pip_bel_idx]
                .find_pin(ResourceName::DeviceResources(src.get_bel()))
                .unwrap_or_else(|| panic!(
                    "Can't find PIP BEL pin for const source BEL `{}`",
                    device.ic_str(src.get_bel())
                ));
            let pip_bel_output_pin_idx = bels[pip_bel_idx]
                .find_pin(create_vconst_net_wire_name(net_name, &mut gsctx))
                .unwrap_or_else(|| panic!(
                    "Can't find PIP BEL pin for const source wire `{}`",
                    device.ic_str(src_wire.get_name())
                ));
            
            let pip_bel_input_belpin =
                tile_belpin_idx[&(pip_bel_idx, pip_bel_input_pin_idx)];
            let pip_bel_output_belpin =
                tile_belpin_idx[&(pip_bel_idx, pip_bel_output_pin_idx)];
                
            /* Connect the BEL pip to virtual site-port BEL */
            graph.connect(from_belpin_idx, pip_bel_input_belpin);

            /* Create pseoudo-pip connection */
            graph.connect(pip_bel_input_belpin, pip_bel_output_belpin);

            /* Create connections to original sinks */
            for sink in sinks {
                graph.connect(pip_bel_output_belpin, sink);
            }
        }
    }

    fn create_routing_graph<'d>(
        device: &Device<'d>,
        st: &crate::ic_loader::archdef::SiteTypeReader<'d>,
        bels: &[BELInfo],
        bel_name_to_bel_idx: &HashMap<ResourceName, usize>,
        site_belpin_idx: &HashMap<(usize, usize), usize>,
        add_virtual_consts: bool
    )
        -> RoutingGraph
    {
        let mut graph = RoutingGraph::new(site_belpin_idx.len());

        Self::init_bels_in_graph(&mut graph, bels, site_belpin_idx);
        Self::init_site_wires_in_graph(
            &mut graph,
            st,
            bels,
            bel_name_to_bel_idx,
            site_belpin_idx
        );
       
        Self::init_pseudopips_in_graph(
            &mut graph,
            st,
            bels,
            bel_name_to_bel_idx,
            site_belpin_idx
        );

        if add_virtual_consts {
            Self::init_virtual_wires_in_graph(
                &mut graph,
                device,
                st,
                bels,
                bel_name_to_bel_idx,
                site_belpin_idx
            )
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
        bel_name: &str,
        pin_name: &str
    )
        -> Result<SitePinId, String>
    {
        let st_list = device.get_site_type_list().unwrap();
        let st = st_list.get(self.st_id);

        let mut bel_found = None;

        let gsctx = GlobalStringsCtx::hold();

        (0 .. self.graph.nodes.len())
            .find(|belpin_id| {
                let (bel_id, bel_pin_id) = self.site_belpin_idx_to_bel_pin[*belpin_id];
                if &*self.bels[bel_id].name.get(device, &gsctx) == bel_name {
                    bel_found = Some(bel_id);
                } else {
                    return false;
                } 
                let bel_pin_name = self.bels[bel_id].pins[bel_pin_id].name
                    .get(device, &gsctx);
                &*bel_pin_name == pin_name
            }).ok_or_else(|| match bel_found {
                    Some(bel_id) => format!(
                        "Pin {}/{}.{} not found",
                        device.ic_str(st.get_name()),
                        self.bels[bel_id].name.get(device, &gsctx),
                        pin_name
                    ),
                    None => format!(
                        "BEL {}/{} not found",
                        device.ic_str(st.get_name()),
                        bel_name
                    ),
                }
            )
            .map(SitePinId)
    }

    pub fn get_pin_name<'d>(
        &'d self,
        device: &Device<'d>,
        gsctx: &'d GlobalStringsCtx,
        pin_id: SitePinId
    )
        -> SitePinName<'d, 'd, impl Borrow<str> + 'd, impl Borrow<str> + 'd>
    {
        let (bel_id, bel_pin_id) = self.site_belpin_idx_to_bel_pin[pin_id.0];
        let bel = self.bels[bel_id].name.get(device, gsctx);
        let pin = self.bels[bel_id].pins[bel_pin_id].name.get(device, gsctx);

        return SitePinName::new(bel, pin)
    }

    pub fn route_pins(
        &self,
        from: SitePinId,
        optimize: bool
    )
        -> impl Iterator<Item = PinPairRoutingInfo>
    {
        let router = PortToPortRouter::<A>::new(&self.graph, from, &self.callback, optimize);
        router.route_all()
            .into_iter()
            .map(move |mut marker| {
                if optimize {
                    marker.constraints = marker.constraints.optimize()
                }
                marker
            })
            .map(Into::into)
    }

    fn route_range(&self, range: std::ops::Range<SitePinId>, optimize: bool)
        -> HashMap<(SitePinId, SitePinId), PinPairRoutingInfo>
    {
        let mut pin_to_pin_map = HashMap::new();
        if range.is_empty() {
            return pin_to_pin_map;
        }

        let pin_cnt = self.graph.nodes.len();
        debug_assert!(range.start < range.end);
        debug_assert!(range.start.0 <= pin_cnt);
        debug_assert!(range.end <= range.end);

        /* XXX: std::iter::Step is experimental, but required to iterate elegantly */
        for from in range.start.0 .. range.end.0 {
            if let PinDir::Input = self.graph.get_node(from).dir {
                continue; /* We don't need routing information for input pins */
            }
            dbg_log!(DBG_EXTRA1, "Routing from pin {}/{}", from, pin_cnt);
            let routing_results = self.route_pins(SitePinId(from), optimize);
            for (to, routing_info) in routing_results.enumerate() {
                if to == from { continue; }
                if (routing_info.requires.len() != 0) || (routing_info.implies.len() != 0) {
                    pin_to_pin_map.insert((SitePinId(from), SitePinId(to)), routing_info);
                }
            }
        }
        pin_to_pin_map
    }

    fn gather_out_of_site_info(
        &self,
        map: &HashMap<(SitePinId, SitePinId), PinPairRoutingInfo>
    )
        -> (HashMap<SitePinId, Vec<SitePinId>>, HashMap<SitePinId, Vec<SitePinId>>)
    {
        let mut out_of_site_sources = HashMap::new();
        let mut out_of_site_sinks = HashMap::new();

        for ((from, to), _) in map {
            let from_node = self.graph.get_node(from.0);
            if let PinDir::Output | PinDir::Inout = from_node.dir {
                if let RoutingGraphNodeKind::SitePort(_) = from_node.kind {
                    out_of_site_sources.entry(*to).or_insert_with(Vec::new).push(*from);
                }
            }

            let to_node = self.graph.get_node(to.0);
            if let PinDir::Input | PinDir::Inout = to_node.dir {
                if let RoutingGraphNodeKind::SitePort(_) = to_node.kind {
                    out_of_site_sinks.entry(*from).or_insert_with(Vec::new).push(*to);
                }
            }
        }

        (out_of_site_sources, out_of_site_sinks)
    }

    pub fn route_all(&self, optimize: bool) -> RoutingInfo {
        let map = self.route_range(
            SitePinId(0) .. SitePinId(self.graph.node_count()),
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

    pub fn create_dot_exporter<'s>(&'s self)
        -> SiteRoutingGraphDotExporter<
            &'s RoutingGraph,
            &'s Vec<BELInfo>,
            &'s Vec<(usize, usize)>
           >
        {
        SiteRoutingGraphDotExporter::new(
            &self.graph,
            &self.bels,
            &self.site_belpin_idx_to_bel_pin
        )
    }
}

pub trait MultiThreadedBruteRouter<A> {
    fn route_all_multithreaded(self, thread_count: usize, optimize: bool) -> RoutingInfo;
}

impl<R, A> MultiThreadedBruteRouter<A> for R
where
    R: Borrow<BruteRouter<A>> + Clone + Send + 'static,
    A: Default + Clone + std::fmt::Debug + 'static
{
    /* Not the best multithreading, but should improve the runtime nevertheless. */
    fn route_all_multithreaded(self, thread_count: usize, optimize: bool) -> RoutingInfo
    {
        let mut total_map = HashMap::new();
        let mut handles = Vec::new();
        
        let pin_cnt = self.borrow().site_belpin_idx_to_bel_pin.len();

        for range in split_range_nicely(0 .. pin_cnt, thread_count) {
            let me = self.clone();
            let handle = thread::spawn(move || {
                me.borrow().route_range(
                    SitePinId(range.start) .. SitePinId(range.end),
                    optimize
                )
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
