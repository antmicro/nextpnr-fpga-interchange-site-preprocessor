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
use std::collections::HashMap;
use std::marker::PhantomData;

use crate::common::IcStr;
use crate::ic_loader::archdef::Root as Device;
use crate::log::*;
use crate::strings::*;

pub mod site_brute_router;
pub mod serialize;

/* XXX: crate::ic_loader::LogicalNetlist_capnp::netlist::Direction doe not implement Hash */
/// Represents a direction of a pin.
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

/// Represents the role of the BEL within a site.
/// 
/// **NOTE**:
/// We do not distinguish between Logic and Routing categories, because some 
/// logic bels can also be route-throughs, and more precise routing information 
/// can be deduced by examining site-pips (pseudo-pips).
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum BELCategory {
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

/// Represents a single pin of a BEL.
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct BELPin {
    pub name: ResourceName,
    pub dir: PinDir,
}

/// Identifies a string associated with any FPGA resource. The string can be stored
/// either in DeviceResources, or in a runtime global string pool.
#[derive(PartialEq, Eq, Hash, Debug, Copy, Clone)]
pub enum ResourceName {
    /// Names loaded from fpga-interchange data
    DeviceResources(u32),
    /// Names created by NISP
    Virtual(GlobalStringId), 
}

#[derive(Debug)]
enum ResourceNameRef<'s> {
    DeviceResources(&'s str),
    Virtual(GlobalStringRef<'s>),
}

impl<'s> std::ops::Deref for ResourceNameRef<'s> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        match self {
            ResourceNameRef::DeviceResources(r) => r,
            ResourceNameRef::Virtual(v) => v.deref(),
        }
    }
}

impl<'s> Borrow<str> for ResourceNameRef<'s> {
    fn borrow(&self) -> &str {
        match self {
            ResourceNameRef::DeviceResources(r) => r.borrow(),
            ResourceNameRef::Virtual(v) => v.borrow(),
        }
    }
}

impl<'s> std::fmt::Display for ResourceNameRef<'s> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceNameRef::DeviceResources(r) => r.fmt(f),
            ResourceNameRef::Virtual(v) => v.fmt(f),
        }
    }
}

impl ResourceName {
    /// Access `str` data associated with the resource name identifier.
    /// Returns a container that allows to reference and borrow the its contents as `str`.
    pub fn get<'s, 'd: 's>(&self, device: &Device<'d>, vctx: &'s GlobalStringsCtx)
        -> impl
            std::ops::Deref<Target = str> +
            std::fmt::Display +
            std::fmt::Debug + 
            std::borrow::Borrow<str> + 's
    {
        match self {
            ResourceName::DeviceResources(id) =>
                ResourceNameRef::DeviceResources(device.ic_str(*id).unwrap()),
            ResourceName::Virtual(id) =>
                ResourceNameRef::Virtual(vctx.get_global_string(*id)),
        }
    }
}

/// Describes a single BEL
pub struct BELInfo {
    pub site_type_idx: u32, /* Site Type Idx IN TILE TYPE! */
    pub name: ResourceName,
    pub category: BELCategory,
    pub pins: Vec<BELPin>,
}

fn create_input_port_bel(
    name: String,
    stitt_id: u32,
)
    -> BELInfo
{
    BELInfo {
        site_type_idx: stitt_id,
        name: ResourceName::Virtual(create_global_string(name.clone())),
        category: BELCategory::SitePort,
        pins: vec![
            BELPin {
                name: ResourceName::Virtual(create_global_string(name)),
                dir: PinDir::Input,
            }
        ],
    }
}

/// Creates a BEL that acts as a pseudo-pip
/// 
/// ```no_run
///        ┏━━━━━┓
/// input──┃ PIP ┃──output
///        ┗━━━━━┛
/// ```
/// # Arguments
/// * `name` - Name that should be given to the _BEL_
/// * `input` - Name that should be given to the input pin
/// * `output` - Name that should be given to the output pin
/// # Return
/// A closure (factory) that builds a _BEL_ given a _stitt_ (Site-Type-In-Tile-Type -
/// identifies the site instance in a tile type)
fn create_pip_bel(
    name: ResourceName,
    input: ResourceName,
    output: ResourceName,
)
    -> impl Fn(u32) -> BELInfo
{
    move |stitt_id: u32| {
        BELInfo {
            site_type_idx: stitt_id,
            name,
            category: BELCategory::LogicOrRouting,
            pins: vec![
                BELPin {
                    name: input,
                    dir: PinDir::Input,
                },
                BELPin {
                    name: output,
                    dir: PinDir::Output,
                }
            ],
        }
    }
}

/// Gathers BELs in the order matching the one in chipdb
/// 
/// See `site_brute_router::BruteRouter::new` for a detailed explanation of the
/// `add_virtual_consts` parameter.
fn gather_bels_in_tile_type<'a>(
    device: &'a Device<'a>,
    tt: &crate::ic_loader::archdef::TileTypeReader<'a>,
    add_virtual_consts: bool
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
                    name: ResourceName::DeviceResources(reader.get_name()),
                    category: reader.get_category().unwrap().into(),
                    pins: reader.get_pins().unwrap().into_iter()
                        .map(|pin_idx| {
                            let pin = st.get_bel_pins().unwrap().get(pin_idx);
                            BELPin {
                                name: ResourceName::DeviceResources(pin.get_name()),
                                dir: pin.get_dir().unwrap().into()
                            }
                        })
                        .collect(),
                })
        );
    }

    if add_virtual_consts {
        use crate::ic_loader::DeviceResources_capnp::device::ConstantType;

        let site_sources = device.get_constants().unwrap().get_site_sources().unwrap();

        for (stitt_id, stitt) in tt.get_site_types().unwrap().iter().enumerate() {
            let st_id = stitt.get_primary_type();

            let st = device.get_site_type_list().unwrap().get(st_id);
            let sw_list = st.get_site_wires().unwrap();
            
            /* For some reason everything in constants is referenced by names */
            let bel_pin_to_wire = st.get_site_wires().unwrap().iter()
                .enumerate()    
                .fold(HashMap::new(), |mut map, (wire_id, wire)| {
                    map.extend(wire.get_pins().unwrap().iter()
                        .map(|pin| {
                            let bpin = st.get_bel_pins().unwrap().get(pin);
                            
                            ((bpin.get_bel(), bpin.get_name()), wire_id as u32)
                        }));
                    map
                });

            let (vcc_added, gnd_added) = site_sources.iter()
                .filter(|site_source| {
                    site_source.get_site_type() == st.get_name()
                })
                .fold((false, false), |(vcc_added, gnd_added), site_source| {
                    let wire_id = bel_pin_to_wire[&(
                        site_source.get_bel(),
                        site_source.get_bel_pin()
                    )];
                    let wire_name = device.ic_str(sw_list.get(wire_id).get_name())
                        .unwrap();
                    let bel_name = device.ic_str(site_source.get_bel()).unwrap();

                    let acc = match site_source.get_constant().unwrap() {
                        ConstantType::Vcc => if !vcc_added {
                            bels.push(create_input_port_bel(
                                "$VCC".into(),
                                stitt_id as u32
                            ));
                            (true, gnd_added)
                        } else {
                            (vcc_added, gnd_added)
                        },
                        ConstantType::Gnd => if !gnd_added {
                            bels.push(create_input_port_bel(
                                "$GND".into(),
                                stitt_id as u32
                            ));
                            (vcc_added, true)
                        } else {
                            (vcc_added, gnd_added)
                        },
                        u @ _ => panic!("Unexpected constant type `{:?}`", u),
                    };

                    bels.push(create_pip_bel(
                        ResourceName::Virtual(create_global_string(
                            format!(
                                "{}_{}", bel_name, wire_name
                            )
                        )),
                        ResourceName::DeviceResources(site_source.get_bel()),
                        ResourceName::DeviceResources(
                            sw_list.get(wire_id).get_name()
                        )
                    )(stitt_id as u32));
                    
                    acc
                });
            
            if vcc_added {
                dbg_log!(
                    DBG_INFO,
                    "  Added $VCC input to site {}_{}",
                    device.ic_str(st.get_name()).unwrap(),
                    stitt_id
                );
            }
            if gnd_added {
                dbg_log!(
                    DBG_INFO,
                    "  Added $GND input to site {}_{}",
                    device.ic_str(st.get_name()).unwrap(),
                    stitt_id
                );
            }
        }
    }

    bels
}

#[derive(Serialize)]
pub struct FullRoutingInfo<I> where I: serde::Serialize {
    pub intra: I,
}

/// Uniquely identifies a site pin within a given tile type.
#[derive(Copy, Clone, Serialize, Debug, Hash, PartialEq, Eq)]
pub struct TilePinId(usize);

/// Holds various name components of a site pin within a tile type.
pub struct TilePinName<'t, 'b, 'p, S, B, P> where
    S: Borrow<str> + 't,
    B: Borrow<str> + 'b,
    P: Borrow<str> + 'p,
{
    site_instance: S,
    bel: B,
    pin: P,
    _tile_lifetime: PhantomData<&'t ()>,
    _bel_lifetime: PhantomData<&'b ()>,
    _pin_lifetime: PhantomData<&'p ()>,
}

impl<'t, 'b, 'p, S, B, P>  TilePinName<'t, 'b, 'p, S, B, P> where
    S: Borrow<str> + 't,
    B: Borrow<str> + 'b,
    P: Borrow<str> + 'p
{
    fn new(site_instance: S, bel: B, pin: P) -> Self {
        Self {
            site_instance,
            bel,
            pin,
            _tile_lifetime: Default::default(),
            _bel_lifetime: Default::default(),
            _pin_lifetime: Default::default(),
        }
    }
}

impl<'t, 'b, 'p, S, B, P> ToString for TilePinName<'t, 'b, 'p, S, B, P> where
    S: Borrow<str> + 't,
    B: Borrow<str> + 'b,
    P: Borrow<str> + 'p
{
    fn to_string(&self) -> String {
        format!(
            "{}/{}.{}",
            self.site_instance.borrow(),
            self.bel.borrow(),
            self.pin.borrow()
        )
    }
}
