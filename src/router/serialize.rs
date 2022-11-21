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
use std::borrow::Borrow;
use super::*;
use super::site_brute_router::PinPairRoutingInfo;


fn map_routing_map_to_serializable<'h, S>(
    routing_map: &'h HashMap<(usize, usize), S>)
    -> HashMap<String, &'h S>
{
    routing_map.iter()
        .map(|(k, v)| (format!("{}->{}", k.0, k.1), v))
        .collect()    
}

fn serialize_standard_routing_info_fields<S, P>(
    ri: &site_brute_router::RoutingInfo<P>,
    ser: &mut S,
) -> Result<(), S::Error>
where
    S: serde::ser::SerializeStruct,
    P: Serialize
{
    let serializable_map = map_routing_map_to_serializable(&ri.pin_to_pin_routing);
        
    ser.serialize_field("pin_to_pin_routing", &serializable_map)?;
    ser.serialize_field("out_of_site_sources", &ri.out_of_site_sources)?;
    ser.serialize_field("out_of_site_sinks", &ri.out_of_site_sinks)?;

    Ok(())
}

impl<P> Serialize for site_brute_router::RoutingInfo<P> where P: Serialize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where
        S: Serializer
    {
        let mut s = serializer.serialize_struct("RoutingInfo", 3)?;
        serialize_standard_routing_info_fields(self, &mut s)?;
        s.end()
    }
}

fn serialize_standard_pin_pair_routing_info_fields<S>(
    ppri: &site_brute_router::PinPairRoutingInfo,
    ser: &mut S,
) -> Result<(), S::Error>
where
    S: serde::ser::SerializeStruct
{
    ser.serialize_field("requires", &ppri.requires)?;
    ser.serialize_field("implies", &ppri.implies)?;

    Ok(())
}

pub struct PinPairRoutingInfoWithDebugInfo {
    from: String,
    to: String,
    search_id: String,
    ppri: site_brute_router::PinPairRoutingInfo
}

impl PinPairRoutingInfoWithDebugInfo {
    pub fn from_ppri<'d, R, A>(
        ppri: site_brute_router::PinPairRoutingInfo,
        brouter: R,
        from: TilePinId,
        to: TilePinId,
        device: &Device<'d>
    ) -> Self
    where
        R: Borrow<site_brute_router::BruteRouter<A>>,
        A: Default + Clone + 'static
    {
        let from = brouter.borrow().get_pin_name(device, from).to_string();
        let to = brouter.borrow().get_pin_name(device, to).to_string();
        Self {
            search_id: format!("{}->{}", from, to),
            from,
            to,
            ppri
        }
    }
}

impl Serialize for PinPairRoutingInfoWithDebugInfo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where
        S: Serializer
    {
        let mut s = serializer.serialize_struct("PinPairRoutingInfo", 6)?;
        s.serialize_field("from", &self.from)?;
        s.serialize_field("to", &self.to)?;
        s.serialize_field("search_id", &self.search_id)?;
        serialize_standard_pin_pair_routing_info_fields(&self.ppri, &mut s)?;
        s.end()
    }
}

pub trait RoutingInfoWithDebugInfo {
    fn with_debug_info<'d, R, A>(self, brouter: R, device: &Device<'d>)
        -> site_brute_router::RoutingInfo<PinPairRoutingInfoWithDebugInfo>
    where
        R: Borrow<site_brute_router::BruteRouter<A>>,
        A: Default + Clone + 'static;
}

impl RoutingInfoWithDebugInfo for site_brute_router::RoutingInfo<PinPairRoutingInfo> {
    fn with_debug_info<'d, R, A>(self, brouter: R, device: &Device<'d>)
        -> site_brute_router::RoutingInfo<PinPairRoutingInfoWithDebugInfo>
    where
        R: Borrow<site_brute_router::BruteRouter<A>>,
        A: Default + Clone + 'static
    {
        site_brute_router::RoutingInfo {
            pin_to_pin_routing: self.pin_to_pin_routing.into_iter()
                .map(|((from, to), v)| {
                    ((from, to), PinPairRoutingInfoWithDebugInfo::from_ppri(
                        v, brouter.borrow(), TilePinId(from), TilePinId(to), device)
                    )
                })
                .collect(),
            out_of_site_sinks: self.out_of_site_sinks,
            out_of_site_sources: self.out_of_site_sources
        }
    }
}
