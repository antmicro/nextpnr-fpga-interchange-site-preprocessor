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


use serde::{Serialize, Serializer, ser::{SerializeStruct, SerializeMap, SerializeSeq}};
use std::collections::HashMap;
use crate::logic_formula::{DNFCube, FormulaTerm};
use std::sync::Arc;

use super::*;


fn serialize_standard_routing_info_fields<'r, 'd, A, S>(
    ri: &RoutingInfoWithExtras<'d, A>,
    ser: &mut S,
) -> Result<(), S::Error>
where
    A: Default + Clone + std::fmt::Debug + 'static,
    S: serde::ser::SerializeStruct,
{
    let serializable_map =
        ri.map_routing_map_to_serializable(&ri.pin_to_pin_routing);
        
    ser.serialize_field("pin_to_pin_routing", &serializable_map)?;
    ser.serialize_field("out_of_site_sources", &ri.out_of_site_sources)?;
    ser.serialize_field("out_of_site_sinks", &ri.out_of_site_sinks)?;

    Ok(())
}

impl<'r, 'd, A> Serialize for RoutingInfoWithExtras<'d, A> where
    A: Default + Clone + std::fmt::Debug + 'static
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where
        S: Serializer
    {
        let mut s = serializer.serialize_struct("RoutingInfo", 3)?;
        serialize_standard_routing_info_fields(self, &mut s)?;
        s.end()
    }
}

pub struct PinPairRoutingInfoWithExtras<'d, A> where 
    A: Default + Clone + std::fmt::Debug
{
    device: &'d Device<'d>,
    router: Arc<site_brute_router::BruteRouter<A>>,
    ppri: site_brute_router::PinPairRoutingInfo
}

#[derive(PartialOrd, Ord, PartialEq, Eq, Serialize)]
pub enum StringConstrainingElement {
    Port(String)
}

impl<'d, A> PinPairRoutingInfoWithExtras<'d, A> where
    A: Default + Clone + std::fmt::Debug + 'static
{
    fn dnf_to_serializable(
        &self,
        form: &[DNFCube<site_brute_router::ConstrainingElement>]
    )
        -> Vec<Vec<FormulaTerm<StringConstrainingElement>>>
    {
        use site_brute_router::ConstrainingElement::*;

        let gsctx = GlobalStringsCtx::hold();

        form.iter().map(|cube| {
            cube.terms.iter().map(|term| {
                term.clone().map(|c| match c {
                    Port(v) => StringConstrainingElement::Port(
                        self.router.get_pin_name(self.device, &gsctx, SitePinId(v as usize))
                            .to_string()
                    )
                })
            }).collect()
        }).collect()
    }
}

impl<'d, A> Serialize for PinPairRoutingInfoWithExtras<'d, A> where
    A: Default + Clone + std::fmt::Debug + 'static
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer
    {
        let mut s = serializer.serialize_struct("PinPairRoutingInfo", 2)?;
        s.serialize_field("requires", &self.dnf_to_serializable(&self.ppri.requires))?;
        s.serialize_field("implies", &self.dnf_to_serializable(&self.ppri.implies))?;
        s.end()
    }
}

pub struct SitePinHashMap<'d, A, T> where
    A: Default + Clone + std::fmt::Debug + 'static
{
    router: Arc<site_brute_router::BruteRouter<A>>,
    device: &'d Device<'d>,
    hashmap: HashMap<SitePinId, T>,
}

impl<'d, A, T> Serialize for SitePinHashMap<'d, A, T> where
    T: Serialize,
    A: Default + Clone + std::fmt::Debug + 'static
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer
    {
        let gsctx = GlobalStringsCtx::hold();
        let mut s = serializer.serialize_map(Some(self.hashmap.len()))?;
        for (key, value) in &self.hashmap {
            s.serialize_entry(
                &self.router.get_pin_name(self.device, &gsctx, *key).to_string(),
                value
            )?;
        }
        s.end()
    }
}

pub struct SitePinVec<'d, A> where
    A: Default + Clone + std::fmt::Debug + 'static
{
    router: Arc<site_brute_router::BruteRouter<A>>,
    device: &'d Device<'d>,
    vec: Vec<SitePinId>,
}

impl<'d, A> Serialize for SitePinVec<'d, A> where
    A: Default + Clone + std::fmt::Debug + 'static
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where
        S: Serializer
    {
        let gsctx = GlobalStringsCtx::hold();
        let mut s = serializer.serialize_seq(Some(self.vec.len()))?;
        for pin in &self.vec {
            s.serialize_element(
                &self.router.get_pin_name(self.device, &gsctx, *pin).to_string()
            )?;
        }
        s.end()
    }
}

pub struct RoutingInfoWithExtras<'d, A> where
    A: Default + Clone + std::fmt::Debug + 'static
{
    router: Arc<site_brute_router::BruteRouter<A>>,
    device: &'d Device<'d>,
    pub pin_to_pin_routing:
        HashMap<(SitePinId, SitePinId), PinPairRoutingInfoWithExtras<'d, A>>,
    pub out_of_site_sources: SitePinHashMap<'d, A, SitePinVec<'d, A>>,
    pub out_of_site_sinks: SitePinHashMap<'d, A, SitePinVec<'d, A>>,
}

impl<'d, A> RoutingInfoWithExtras<'d, A> where
    A: Default + Clone + std::fmt::Debug + 'static
{
    fn map_routing_map_to_serializable<'h, S>(
        &self,
        routing_map: &'h HashMap<(SitePinId, SitePinId), S>
    )
        -> HashMap<String, &'h S>
    {
        let gsctx = GlobalStringsCtx::hold();

        routing_map.iter()
            .map(|((from, to), v)| {
                let from_name =
                    self.router.get_pin_name(self.device, &gsctx, *from);
                let to_name =
                    self.router.get_pin_name(self.device, &gsctx, *to);
                (format!("{}->{}", from_name.to_string(), to_name.to_string()), v)
            })
            .collect()    
    }

    fn convert_hashmap(
        router: Arc<site_brute_router::BruteRouter<A>>,
        device: &'d Device<'d>,
        hm: HashMap<SitePinId, Vec<SitePinId>>
    )
        -> SitePinHashMap<'d, A, SitePinVec<'d, A>>
    {
        let router_c = Arc::clone(&router);
        SitePinHashMap {
            router,
            device,
            hashmap: hm.into_iter().map(|(k, v)| {
                let vec = SitePinVec {
                    router: Arc::clone(&router_c),
                    device,
                    vec: v
                };

                (k, vec)
            }).collect()
        }
    }
}

pub trait IntoRoutingInfoWithExtras<'d, A> where
    A: Default + Clone + std::fmt::Debug + 'static
{
    fn with_extras(
        self,
        router: Arc<site_brute_router::BruteRouter<A>>,
        device: &'d Device<'d>
    )
        -> RoutingInfoWithExtras<'d, A>;
}

impl<'d, A> IntoRoutingInfoWithExtras<'d, A> for site_brute_router::RoutingInfo where
    A: Default + Clone + std::fmt::Debug + 'static
{
    fn with_extras(
        self,
        router: Arc<site_brute_router::BruteRouter<A>>,
        device: &'d Device<'d>
    )
        -> RoutingInfoWithExtras<'d, A>
    {
        let router_ref = &router;
        let ptpr: HashMap::<_, _> = self.pin_to_pin_routing.into_iter()
            .map(move |(key, ppri)|
                (key, PinPairRoutingInfoWithExtras {
                    router: Arc::clone(router_ref),
                    device,
                    ppri
                })
            ).collect();
        
        RoutingInfoWithExtras {
            router: Arc::clone(&router),
            device,
            pin_to_pin_routing: ptpr,
            out_of_site_sources: RoutingInfoWithExtras::convert_hashmap(
                Arc::clone(&router),
                device,
                self.out_of_site_sources
            ),
            out_of_site_sinks: RoutingInfoWithExtras::convert_hashmap(
                Arc::clone(&router),
                device,
                self.out_of_site_sinks
            ),
        }
    }
}
