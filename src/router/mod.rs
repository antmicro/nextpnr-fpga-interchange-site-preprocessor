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

use crate::ic_loader::archdef::Root as Device;

pub mod site_brute_router;

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

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum BELCategory {
    /* We do not distinguish between Logic and Routing categories, because some 
     * logic bels can also be route-throughs, and more precise routing information 
     * can be deduced by examining site-pips (pseudo-pips). */
    LogicOrRouting,
    SitePort,
}

impl From<crate::ic_loader::DeviceResources_capnp::device::BELCategory> for BELCategory {
    fn from(cat: crate::ic_loader::DeviceResources_capnp::device::BELCategory) -> Self {
        use crate::ic_loader::DeviceResources_capnp::device::BELCategory::*;
        match cat {
            Logic => Self::LogicOrRouting,
            Routing => Self::LogicOrRouting,
            SitePort => Self::SitePort,
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct BELPin {
    pub idx_in_site_type: u32,
    pub name: u32,
    pub dir: PinDir,
}

pub struct BELInfo {
    pub site_type_idx: u32, /* Site Type Idx IN TILE TYPE! */
    pub name: u32,
    pub category: BELCategory,
    pub pins: Vec<BELPin>,
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
                    category: reader.get_category().unwrap().into(),
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
