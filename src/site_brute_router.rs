use std::collections::{HashMap, VecDeque};
use crate::common::IcStr;
use crate::logic_formula::*;
use lazy_static::__Deref;
use replace_with::replace_with_or_abort;
use std::thread;
use crate::log::*;
use crate::ic_loader::archdef::Root as Device;
use serde::{Serialize, Deserialize};

/* XXX: crate::ic_loader::LogicalNetlist_capnp::netlist::Direction doe not implement Hash */
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum PinDir {
    Inout,
    Input,
    Output,
}

impl From<crate::ic_loader::LogicalNetlist_capnp::netlist::Direction> for PinDir {
    fn from(pd: crate::ic_loader::LogicalNetlist_capnp::netlist::Direction) -> Self {
        use crate::ic_loader::LogicalNetlist_capnp::netlist::Direction::*;
        match pd {
            Inout => Self::Inout,
            Input => Self::Input,
            Output => Self::Output,
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct BELPin {
    pub idx_in_site_type: u32,
    pub name: u32,
    pub dir: PinDir,
}

struct BELInfo {
    site_type_idx: u32, /* Site Type Idx IN TILE TYPE! */
    name: u32,
    pins: Vec<BELPin>,
}

#[derive(Serialize)]
pub struct RoutingInfo {
    route_constraintes: Vec<DNFCube<ConstrainingElement>>,
}

impl RoutingInfo {
    /* A primitive heuristic for sorting constraints by number of terms.
     * The idea is that a greedy algorithm would set value of the least
     * constraints when placing a cell. Perhaps a better heuristic could
     * be found by performing some stochastic process across all routing
     * infos to try to determine which ones collide with each other the
     * least. */
    fn default_sort(&mut self) {
        self.route_constraintes.sort_by_key(|cube| cube.len());
    }
}

impl From<PTPRMarker> for RoutingInfo {
    fn from(marker: PTPRMarker) -> Self {
        let mut me = Self { route_constraintes: marker.constraints.cubes };
        me.default_sort();
        me
    }
}

/* Gathers BELs in the order matching the one in chipdb */
fn gather_bels_in_tile_type<'a>(
    device: &'a Device<'a>,
    tt: &crate::ic_loader::archdef::TileTypeReader<'a>
) -> Vec<BELInfo> {
    let mut bels = Vec::new();
    let st_list = device.reborrow().get_site_type_list().unwrap();
    for (stitt_idx, stitt) in tt.reborrow().get_site_types().unwrap().iter().enumerate() {
        let st_idx = stitt.get_primary_type();
        let st = st_list.get(st_idx);
        bels.extend(
            st.get_bels().unwrap().into_iter()
                .map(|reader| BELInfo {
                    site_type_idx: stitt_idx as u32,
                    name: reader.get_name(),
                    pins: reader.get_pins().unwrap().into_iter()
                        .map(|pin_idx| {
                            let pin = st.get_bel_pins().unwrap().get(pin_idx);
                            BELPin {
                                idx_in_site_type: pin_idx,
                                name: pin.get_name(),
                                dir: pin.get_dir().unwrap().into()
                            }
                        })
                        .collect(),
                })
        );
    }
    bels
}

type RoutingGraphEdge = bool;

#[derive(Clone, Default)]
struct RoutingGraphNode {
    bel: Option<usize>,            /* Edge routed through another BEL */
}

struct RoutingGraph {
    nodes: Vec<RoutingGraphNode>,
    edges: Vec<RoutingGraphEdge>,  /* Edges between BEL pins */
}

impl RoutingGraph {
    fn new(pin_count: usize) -> Self {
        Self {
            nodes: vec![Default::default(); pin_count],
            edges: vec![Default::default(); pin_count * pin_count],
        }
    }

    fn get_edge<'a>(&'a self, from: usize, to: usize) -> &'a RoutingGraphEdge {
        &self.edges[from * self.nodes.len() + to]
    }

    fn get_edge_mut<'a>(&'a mut self, from: usize, to: usize) -> &'a mut RoutingGraphEdge {
        &mut self.edges[from * self.nodes.len() + to]
    }

    fn connect<'a>(&'a mut self, from: usize, to: usize)
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

    fn get_node<'a>(&'a self, node: usize) -> &'a RoutingGraphNode {
        &self.nodes[node]
    }

    fn get_node_mut<'a>(&'a mut self, node: usize) -> &'a mut RoutingGraphNode {
        &mut self.nodes[node]
    }

    fn edges_from<'a>(&'a self, from: usize) -> impl Iterator<Item = usize> + 'a {
        self.edges.iter()
            .skip(from * self.nodes.len())
            .take(self.nodes.len())
            .enumerate()
            .filter(|(_, e)| **e)
            .map(|(idx, _)| idx)
    }

    fn edges_to<'a>(&'a self, to: usize) -> impl Iterator<Item = usize> + 'a {
        self.edges.iter()
            .skip(to)
            .step_by(self.nodes.len())
            .take(self.nodes.len())
            .enumerate()
            .filter(|(_, e)| **e)
            .map(|(idx, _)| idx)
    }
}

#[derive(PartialOrd, PartialEq, Ord, Eq, Clone, Debug, Serialize, Deserialize)]
enum ConstrainingElement {
    Port(u32),
    ClockSignalPolarity(u32),
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
}

impl<'g> PortToPortRouter<'g> {
    fn new(graph: &'g RoutingGraph, from: usize) -> Self {
        Self {
            graph,
            from,
            markers: (0 .. graph.nodes.len()).map(|_| {
                PTPRMarker {
                    /* visited: 0, */
                    constraints: DNFForm::new(),
                }
            }).collect(),
            queue: VecDeque::new(),
        }
    }

    fn routing_step(&mut self, previous: Option<usize>) -> Option<usize> {
        let (previous_node, current_node) = self.queue.pop_front()?;

        self.scan_and_add_constraints(current_node, previous_node);
        
        for next in self.graph.edges_from(current_node) {
            let is_subformular = {
                let my_constr = &self.markers[current_node].constraints;
                let next_constr = &self.markers[next].constraints;
                my_constr.is_subformula_of(next_constr)
            };
            
            if !is_subformular {
                let my_constr = self.markers[current_node].constraints.clone();
                replace_with_or_abort(&mut self.markers[next].constraints, |c| {
                    c.disjunct(my_constr)
                });
                self.queue.push_back((Some(current_node), next));
            }
        }

        Some(current_node)
    }

    fn scan_and_add_constraints(&mut self, node: usize, previous: Option<usize>) {
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
            DBG_EXTRA,
            "Added constraints for node {}. Current status: {:?}",
            node,
            self.markers[node].constraints
        );
    }

    fn init_constraints(&mut self, node: usize) {
        self.markers[node].constraints = DNFForm::new()
            .add_cube( DNFCube { terms: vec![FormulaTerm::True] })
    }

    fn route_all(mut self, mut step_counter: Option<&mut usize>) -> Vec<PTPRMarker> {
        #[cfg(debug_assertions)]
        if let Some(ref mut counter) = step_counter { **counter = 0 };

        self.init_constraints(self.from);

        self.queue.clear();
        self.queue.push_back((None, self.from));
        let mut previous = None;
        loop {
            #[cfg(debug_assertions)]
            if let Some(ref mut counter) = step_counter { **counter += 1 };

            match self.routing_step(previous) {
                Some(current) => previous = Some(current),
                None => break,
            }
        }
        self.markers
    }
}

pub struct BruteRouter<'a> {
    tt: crate::ic_loader::archdef::TileTypeReader<'a>,
    bels: Vec<BELInfo>,
    tile_belpin_idx_to_bel_pin: Vec<(usize, usize)>,
    pub pin_to_pin_map: HashMap<BELPin, HashMap<BELPin, RoutingInfo>>,
    pub sinks: Vec<BELPin>,
    graph: RoutingGraph,
}

impl<'a> BruteRouter<'a> {
    pub fn new(
        device: &'a Device<'a>,
        tt: &crate::ic_loader::archdef::TileTypeReader<'a>,
    ) -> Self {

        /* Create mappings between elements and indices */
        let bels = gather_bels_in_tile_type(&device, &tt);

        let mut pin_to_pin_map = HashMap::new();
        //let mut drivers = Vec::new();
        let mut sinks = Vec::new();
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

        /* Create routing graph: conections between BELs */
        let mut graph = RoutingGraph::new(tile_belpin_idx.len());
        for (stitt_idx, stitt) in tt.get_site_types().unwrap().iter().enumerate() {
            let site_type_idx = stitt.get_primary_type();
            let site_type = device.reborrow()
                .get_site_type_list().unwrap()
                .get(site_type_idx);
            
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
                
                match graph.get_node_mut(tile_in_pin_idx).bel {
                    ref mut bopt @ None => *bopt = Some(bel_idx),
                    Some(other_bel_idx) => assert!(other_bel_idx == bel_idx),
                }
                match graph.get_node_mut(tile_out_pin_idx).bel {
                    ref mut bopt @ None => *bopt = Some(bel_idx),
                    Some(other_bel_idx) => assert!(other_bel_idx == bel_idx),
                }

                let _ = graph.connect(tile_in_pin_idx, tile_out_pin_idx);
            }
        }

        assert_eq!(tile_belpin_idx_to_bel_pin.len(), graph.nodes.len());

        Self {
            tt: tt.clone(),
            bels,
            pin_to_pin_map,
            sinks,
            tile_belpin_idx_to_bel_pin,
            graph,
        }
    }

    fn route_pins(
        graph: &RoutingGraph,
        from: usize,
        step_counter: Option<&mut usize>, /* Can be used only in debug build */
    ) -> impl Iterator<Item = RoutingInfo> {
        let router = PortToPortRouter::new(graph, from);
        router.route_all(step_counter)
            .into_iter()
            .map(Into::into)
    }

    fn route_range(graph: &RoutingGraph, range: std::ops::Range<usize>)
        -> HashMap<(usize, usize), RoutingInfo>
    {
        let pin_cnt = graph.nodes.len();
        debug_assert!(range.start < range.end);
        debug_assert!(range.start <= pin_cnt);
        debug_assert!(range.end <= range.end);

        let mut pin_to_pin_map = HashMap::new();
        for from in range {
            dbg_log!(DBG_INFO, "Routing from pin {}/{}", from, pin_cnt);
            let mut step_counter = 0;
            let routing_results = Self::route_pins(graph, from, Some(&mut step_counter));
            dbg_log!(DBG_INFO, "  Number of steps: {}", step_counter);
            for (to, routing_info) in routing_results.enumerate() {
                if routing_info.route_constraintes.len() != 0 {
                    pin_to_pin_map.insert((from, to), routing_info);
                }
            }
        }
        pin_to_pin_map
    }

    pub fn route_all(self) -> HashMap<(usize, usize), RoutingInfo> {
        Self::route_range(&self.graph, 0 .. self.tile_belpin_idx_to_bel_pin.len())
    }

    /* Not the best multithreading, but should improve the runtime nevertheless. */
    pub fn route_all_multithreaded(self, thread_count: usize)
        -> HashMap<(usize, usize), RoutingInfo>
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
                Self::route_range(graph, range)
            });
            handles.push(handle);
        }
        for handle in handles {
            let map = handle.join().unwrap();
            total_map.extend(map.into_iter());
        }
        total_map
    }

    /* TODO: This should be in a separate file. */
    /* Exports routing graph in DOT format. */
    pub fn export_dot(&self, device: &Device<'a>, name: &str) -> String {
        let mut bel_subgraphs = HashMap::new();

        /* Group pins of the same BELs into subgraphs */
        let st_list = device.reborrow().get_site_type_list().unwrap();
        for (node_idx, _) in self.graph.nodes.iter().enumerate() {
            let (bel_idx, _) = self.tile_belpin_idx_to_bel_pin[node_idx];
            let stitt = self.bels[bel_idx].site_type_idx;
            let bel_name = device.ic_str(self.bels[bel_idx].name).unwrap();
            let st_idx = self.tt.get_site_types().unwrap().get(stitt).get_primary_type();
            let st = st_list.get(st_idx);
            let st_name = device.ic_str(st.get_name()).unwrap();

            let bel_name = format!("{}_{}/{}", st_name, stitt, bel_name);

            let bucket = bel_subgraphs.entry(bel_name).or_insert_with(|| Vec::new());
            bucket.push(node_idx);
        }
        
        /* Write DOT */
        let mut dot = "# DOT Graph generated by NISP\n\n".to_string();
        dot += &format!("digraph {} {{\n\n", name);

        for (bel_name, bel_pins) in bel_subgraphs {
            

            dot += &format!("    subgraph cluster_{} {{\n", bel_name.replace('/', "__"));
            dot += &format!("        node [style=filled];\n");
            dot += &format!("        label = \"{}\";\n", bel_name);
            dot += &format!("        color = \"blue\";\n");

            for pin_idx in bel_pins {
                //let node = &self.graph.nodes[pin_idx];
                let (bel_idx, bel_pin_idx) = self.tile_belpin_idx_to_bel_pin[pin_idx];
                let bel = &self.bels[bel_idx];
                
                //assert_eq!(inputs.device.ic_str(bel.name).unwrap(), bel_name);
                //let bel_name = inputs.device.ic_str(bel.name).unwrap();
                let pin_name = device.ic_str(bel.pins[bel_pin_idx].name).unwrap();
                
                dot += &format!(
                    "        {} [label=\"{}\"];\n",
                    pin_idx,
                    pin_name
                );
            }
            dot += &format!("    }}\n\n");
        }

        for from in 0 .. self.graph.nodes.len() {
            for to in self.graph.edges_from(from) {
                dot += &format!("    {} -> {};\n", from, to);
            }
        }

        dot += "}\n";

        dot
    }
}

/* Splits a range into `slices` possibly even ranges  */
fn split_range_nicely(range: std::ops::Range<usize>, slices: usize)
    -> impl Iterator<Item = std::ops::Range<usize>> where
{
    let len = range.end - range.start;
    let split_sz = len / slices;
    let total = split_sz * slices;
    let left = len - total;
    
    (0 .. slices)
        .scan((0, left), move |(current_idx, left), _| {
            let my_len = if *left > 0 {
                *left -= 1;
                split_sz + 1
            } else {
                split_sz
            };
            let range = *current_idx .. (*current_idx + my_len);
            *current_idx += my_len;
            return Some(range);
        })
        .filter(|range| range.start != range.end)
}

