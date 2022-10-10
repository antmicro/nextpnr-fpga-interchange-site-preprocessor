use std::collections::HashMap;
use crate::IcStr;

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

struct BELInfo<'a> {
    site_type_idx: u32, /* Site Type Idx IN TILE TYPE! */
    name: u32,
    pins: Vec<BELPin>,
    reader: crate::ic_loader::DeviceResources_capnp::device::b_e_l::Reader<'a>
}

pub struct RoutingInfo(u8);

/* Gathers BELs in the order matching the one in chipdb */
fn gather_bels_in_tile_type<'a>(
    inputs: &'a crate::Inputs<'a>,
    tt: &crate::ic_loader::archdef::TileTypeReader<'a>
) -> Vec<BELInfo<'a>> {
    let mut bels = Vec::new();
    let st_list = inputs.device.reborrow().get_site_type_list().unwrap();
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
                    reader
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
        let mut edge = self.get_edge_mut(from, to);

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
}

pub struct BruteRouter<'a> {
    tt: crate::ic_loader::archdef::TileTypeReader<'a>,
    bels: Vec<BELInfo<'a>>,
    tile_belpin_idx_to_bel_pin: Vec<(usize, usize)>,
    pub pin_to_pin_map: HashMap<BELPin, HashMap<BELPin, RoutingInfo>>,
    pub sinks: Vec<BELPin>,
    graph: RoutingGraph,
}

impl<'a> BruteRouter<'a> {
    pub fn new(
        inputs: &'a crate::Inputs<'a>,
        tt: &crate::ic_loader::archdef::TileTypeReader<'a>,
    ) -> Self {

        /* Create mappings between elements and indices */
        let bels = gather_bels_in_tile_type(&inputs, &tt);

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
                let name = inputs.device.ic_str(bel.name).unwrap();
                let st_list = tt.reborrow().get_site_types().unwrap();
                let other_st = inputs.device.reborrow().get_site_type_list().unwrap()
                    .get(st_list.get(bels[other_idx].site_type_idx).get_primary_type());
                let st = inputs.device.reborrow().get_site_type_list().unwrap()
                    .get(st_list.get(bel.site_type_idx).get_primary_type());
                
                panic!(
                    concat!(
                        "Conflicting BELs in tile type {}! ({}) {} conflicts with {}. ",
                        "Site types are {} and {}."
                    ),
                    inputs.device.ic_str(tt.get_name()).unwrap(),
                    name,
                    bel_idx,
                    other_idx,
                    inputs.device.ic_str(other_st.get_name()).unwrap(),
                    inputs.device.ic_str(st.get_name()).unwrap(),
                );
            }
            for pin_idx in 0 .. bel.pins.len() {
                tile_belpin_idx.insert((bel_idx, pin_idx), belpin_idx);
                tile_belpin_idx_to_bel_pin.push((bel_idx, pin_idx));
                belpin_idx += 1;
            }
        }

        /* Create routing graph: conections between BELs */
        let mut graph = RoutingGraph::new(tile_belpin_idx.len());
        for (stitt_idx, stitt) in tt.get_site_types().unwrap().iter().enumerate() {
            let site_type_idx = stitt.get_primary_type();
            let site_type = inputs.device.reborrow()
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
                        if driver != *sink { /* XXX: driver can equal to sink in case of Inout */
                            let _ = graph.connect(driver, *sink);
                        }
                    }
                }
            }
        }

        /* Create routing graph: add edges for pseudo-pips (routing BELs) */
        for (stitt_idx, stitt) in tt.get_site_types().unwrap().iter().enumerate() {
            let st_idx = stitt.get_primary_type();
            let st = inputs.device.reborrow().get_site_type_list().unwrap().get(st_idx);
            let ic_bel_pins = st.reborrow().get_bel_pins().unwrap();
            for spip in st.get_site_p_i_ps().unwrap() {
                let in_pin_idx = spip.get_inpin();
                let out_pin_idx = spip.get_outpin();
                
                let in_bel_name = ic_bel_pins.get(in_pin_idx).get_bel();
                let out_bel_name = ic_bel_pins.get(out_pin_idx).get_bel();
                /* (Pseudo)PIP should represent a single routing BEL */
                assert!(in_bel_name == out_bel_name);

                let bel_idx = bel_name_to_bel_idx[&(stitt_idx as u32, in_bel_name)];
                match graph.get_node_mut(bel_idx).bel {
                    ref mut bopt @ None => *bopt = Some(bel_idx),
                    Some(other_bel_idx) => assert!(other_bel_idx == bel_idx),
                }

                let _ = graph.connect(
                    spip.get_inpin() as usize,
                    spip.get_outpin() as usize);
            }
        }

        Self {
            tt: tt.clone(),
            bels,
            pin_to_pin_map,
            sinks,
            tile_belpin_idx_to_bel_pin,
            graph,
        }
    }

    /* fn route_pins(&self, from: usize, to: usize) -> RoutingInfo {
        let (from_bel_idx, from_pin_idx) = self.tile_belpin_idx_to_bel_pin[from];
        let (to_bel_idx, to_pin_idx) = self.tile_belpin_idx_to_bel_pin[to];

        let bel = self.bels[from_bel_idx];
    } */
}