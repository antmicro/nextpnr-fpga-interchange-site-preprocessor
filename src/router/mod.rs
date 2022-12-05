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
#[allow(unused)]
use crate::log::*;
use crate::strings::*;
use crate::ic_loader::{DeviceResources_capnp, LogicalNetlist_capnp};

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

impl From<LogicalNetlist_capnp::netlist::Direction> for PinDir {
    fn from(pd: LogicalNetlist_capnp::netlist::Direction) -> Self {
        use LogicalNetlist_capnp::netlist::Direction::*;
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

impl From<DeviceResources_capnp::device::BELCategory> for BELCategory {
    fn from(cat: DeviceResources_capnp::device::BELCategory) -> Self {
        use DeviceResources_capnp::device::BELCategory::*;
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

#[derive(Debug, Hash, PartialEq, Eq)]
pub enum ResourceNameRef<'s> {
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
    pub fn get<'d>(&self, device: &Device<'d>, vctx: &'d GlobalStringsCtx)
        -> ResourceNameRef<'d>
    {
        match self {
            ResourceName::DeviceResources(id) =>
                ResourceNameRef::DeviceResources(device.ic_str(*id)),
            ResourceName::Virtual(id) =>
                ResourceNameRef::Virtual(vctx.get_global_string(*id)),
        }
    }
}

/// Describes a single BEL
pub struct BELInfo {
    pub name: ResourceName,
    pub category: BELCategory,
    pub pins: Vec<BELPin>,
}

impl BELInfo {
    pub fn find_pin(&self, name: ResourceName) -> Option<usize> {
        self.pins.iter()
            .enumerate()
            .find_map(|(pin_idx, pin)| (pin.name == name).then(|| pin_idx))
    }
}

/// Creates a BEL that acts as site's input pin.
/// 
/// The BEL will have a sinlge pin, that will output the input for a site.
/// The pin will be have the same name as the BEL.
/// 
/// ```text
///              ╔══════════════...
///              ║     SITE     ...
///              ║┏━━━━━┓       ...
/// site_pin━━━━━╟┃ PIN ┃──pin  ...
///              ║┗━━━━━┛       ...
///              ╚══════════════...
/// ```
/// 
/// # Arguments
/// * `name` - Name of the pin
fn create_input_port_bel(name: String)
    -> BELInfo
{
    let mut gsctx = GlobalStringsCtx::hold();
    
    BELInfo {
        name: ResourceName::Virtual(gsctx.create_global_string(name.clone())),
        category: BELCategory::SitePort,
        pins: vec![
            BELPin {
                name: ResourceName::Virtual(gsctx.create_global_string(name)),
                dir: PinDir::Output,
            }
        ],
    }
}

/// Creates a rouiting BEL that acts as a pseudo-pip
/// 
/// ```text
///        ┏━━━━━┓
/// input──┃ PIP ┃──output
///        ┗━━━━━┛
/// ```
/// # Arguments
/// * `name` - Name that should be given to the _BEL_
/// * `input` - Name that should be given to the input pin
/// * `output` - Name that should be given to the output pin
fn create_pip_bel(
    name: ResourceName,
    input: ResourceName,
    output: ResourceName,
)
    -> BELInfo
{
    BELInfo {
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

/// Creates a name for routing BEL between $VCC/$GND and constant generator
/// 
/// # Arguments
/// * `bel_name` - name of the generator BEL
/// * `net_name` - name of the constant network ("$VCC"/"$GND")
/// * `gsctx` - global string context
fn create_vconst_net_pipbel_name<B, N>(
    bel_name: B,
    net_name: N,
    gsctx: &mut GlobalStringsCtx
)
    -> ResourceName
where
    B: std::fmt::Display,
    N: std::fmt::Display
{
    ResourceName::Virtual(gsctx.create_global_string(
        format!("{}_{}_SITE_WIRE", bel_name, net_name)
    ))
}

fn create_belname_pinname_to_wire_lookup<'d>(
    st: &DeviceResources_capnp::device::site_type::Reader<'d>
)
    -> HashMap<(u32, u32), u32>
{
    st.get_site_wires().unwrap().iter()
        .enumerate()    
        .fold(HashMap::new(), |mut map, (wire_id, wire)| {
            map.extend(wire.get_pins().unwrap().iter()
                .map(|pin| {
                    let bpin = st.get_bel_pins().unwrap().get(pin);
                    
                    ((bpin.get_bel(), bpin.get_name()), wire_id as u32)
                }));
            map
        })
}

fn gather_bels_in_site_type<'a>(
    device: &'a Device<'a>,
    st: &crate::ic_loader::archdef::SiteTypeReader<'a>,
    add_virtual_consts: bool
) -> Vec<BELInfo> {
    let mut bels: Vec<_> = st.get_bels().unwrap().into_iter()
        .map(|reader| BELInfo {
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
        }).collect();

    if add_virtual_consts {
        use crate::ic_loader::DeviceResources_capnp::device::ConstantType;

        let site_sources = device.get_constants().unwrap().get_site_sources().unwrap();

        let sw_list = st.get_site_wires().unwrap();
        
        /* For some reason everything in constants is referenced by names */
        let bel_pin_to_wire = create_belname_pinname_to_wire_lookup(&st);
        
        let mut gsctx = GlobalStringsCtx::hold();

        let (vcc_added, gnd_added) = site_sources.iter()
            .filter(|site_source| {
                site_source.get_site_type() == st.get_name()
            })
            .fold((false, false), |(vcc_added, gnd_added), site_source| {
                let wire_id = bel_pin_to_wire[&(
                    site_source.get_bel(),
                    site_source.get_bel_pin()
                )];
                let bel_name = device.ic_str(site_source.get_bel());

                let (acc, net_name) = match site_source.get_constant().unwrap() {
                    ConstantType::Vcc => if !vcc_added {
                        bels.push(create_input_port_bel("$VCC".into()));
                        ((true, gnd_added), "$VCC")
                    } else {
                        ((vcc_added, gnd_added), "$VCC")
                    },
                    ConstantType::Gnd => if !gnd_added {
                        bels.push(create_input_port_bel(
                            "$GND".into()
                        ));
                        ((vcc_added, true), "$GND")
                    } else {
                        ((vcc_added, gnd_added), "$GND")
                    },
                    u @ _ => panic!("Unexpected constant type `{:?}`", u),
                };

                bels.push(create_pip_bel(
                    create_vconst_net_pipbel_name(bel_name, net_name, &mut gsctx),
                    ResourceName::DeviceResources(site_source.get_bel()),
                    ResourceName::DeviceResources(
                        sw_list.get(wire_id).get_name()
                    )
                ));
                
                acc
            });
        
        if vcc_added {
            dbg_log!(
                DBG_INFO,
                "  Added $VCC input to site {}",
                device.ic_str(st.get_name())
            );
        }
        if gnd_added {
            dbg_log!(
                DBG_INFO,
                "  Added $GND input to site {}",
                device.ic_str(st.get_name())
            );
        }
    }

    bels
}

/// Contains information about all aspecs of routing
/// 
/// Fields:
/// * `intra` - intra-site routing info
#[derive(Serialize)]
pub struct FullRoutingInfo<I> where I: serde::Serialize {
    pub intra: I,
}

/// Uniquely identifies a site pin within a given site type.
#[derive(Copy, Clone, Serialize, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct SitePinId(usize);

/// Holds various name components of a site pin within a site type.
pub struct SitePinName<'b, 'p, B, P> where
    B: Borrow<str> + 'b,
    P: Borrow<str> + 'p,
{
    bel: B,
    pin: P,
    _bel_lifetime: PhantomData<&'b ()>,
    _pin_lifetime: PhantomData<&'p ()>,
}

impl<'b, 'p, B, P>  SitePinName<'b, 'p, B, P> where
    B: Borrow<str> + 'b,
    P: Borrow<str> + 'p
{
    fn new(bel: B, pin: P) -> Self {
        Self {
            bel,
            pin,
            _bel_lifetime: Default::default(),
            _pin_lifetime: Default::default(),
        }
    }
}

impl<'b, 'p, B, P> ToString for SitePinName<'b, 'p, B, P> where
    B: Borrow<str> + 'b,
    P: Borrow<str> + 'p
{
    fn to_string(&self) -> String {
        format!(
            "{}.{}",
            self.bel.borrow(),
            self.pin.borrow()
        )
    }
}