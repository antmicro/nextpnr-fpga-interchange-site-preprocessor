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


use serde::{Serialize, Serializer, ser::SerializeStruct};
use std::collections::HashMap;
use crate::logic_formula::{DNFCube, FormulaTerm};
use std::sync::Arc;

use super::*;


fn serialize_standard_routing_info_fields<'r, 'd, A, P, S>(
    ri: &RoutingInfoWithExtras<'d, A, P>,
    ser: &mut S,
) -> Result<(), S::Error>
where
    A: Default + Clone + std::fmt::Debug + 'static,
    P: Serialize,
    S: serde::ser::SerializeStruct,
{
    let serializable_map =
        ri.map_routing_map_to_serializable(&ri.info.pin_to_pin_routing);
        
    ser.serialize_field("pin_to_pin_routing", &serializable_map)?;
    ser.serialize_field("out_of_site_sources", &ri.info.out_of_site_sources)?;
    ser.serialize_field("out_of_site_sinks", &ri.info.out_of_site_sinks)?;

    Ok(())
}

pub struct RoutingInfoWithExtras<'d, A, P> where
    A: Default + Clone + std::fmt::Debug + 'static,
    P: Serialize
{
    device: &'d Device<'d>,
    router: Arc<site_brute_router::BruteRouter<A>>,
    info: site_brute_router::RoutingInfo<P>,
    _a: std::marker::PhantomData<A>,
}

impl<'d, A, P> RoutingInfoWithExtras<'d, A, P> where
    A: Default + Clone + std::fmt::Debug + 'static,
    P: Serialize
{
    fn map_routing_map_to_serializable<'h, S>(
        &self,
        routing_map: &'h HashMap<(usize, usize), S>
    )
        -> HashMap<String, &'h S>
    {
        let gsctx = GlobalStringsCtx::hold();

        routing_map.iter()
            .map(|(k, v)| {
                let from_name =
                    self.router.get_pin_name(self.device, &gsctx, SitePinId(k.0));
                let to_name =
                    self.router.get_pin_name(self.device, &gsctx, SitePinId(k.1));
                (format!("{}->{}", from_name.to_string(), to_name.to_string()), v)
            })
            .collect()    
    }
}

impl<'r, 'd, A, P> Serialize for RoutingInfoWithExtras<'d, A, P> where
    A: Default + Clone + std::fmt::Debug + 'static,
    P: Serialize
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
    ppri: site_brute_router::PinPairRoutingInfo<site_brute_router::ConstrainingElement>
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

pub trait IntoRoutingInfoWithExtras<'d, A>
where
    A: Default + Clone + std::fmt::Debug + 'static
{
    fn with_extras(
        self,
        router: Arc<site_brute_router::BruteRouter<A>>,
        device: &'d Device<'d>
    )
        -> RoutingInfoWithExtras<'d, A, PinPairRoutingInfoWithExtras<'d, A>>;
}

impl<'d, A> IntoRoutingInfoWithExtras<'d, A> for
    site_brute_router::RoutingInfo<
        site_brute_router::PinPairRoutingInfo<site_brute_router::ConstrainingElement>
    >
where
    A: Default + Clone + std::fmt::Debug + 'static
{
    fn with_extras(
        self,
        router: Arc<site_brute_router::BruteRouter<A>>,
        device: &'d Device<'d>
    )
        -> RoutingInfoWithExtras<'d, A, PinPairRoutingInfoWithExtras<'d, A>>
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
            device,
            router: router,
            info: site_brute_router::RoutingInfo {
                pin_to_pin_routing: ptpr,
                out_of_site_sources: self.out_of_site_sources,
                out_of_site_sinks: self.out_of_site_sinks,
            },
            _a: Default::default() }
    }
}
