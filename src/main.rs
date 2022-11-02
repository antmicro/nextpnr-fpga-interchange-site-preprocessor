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


use clap::Parser;
use serde::Serialize;
use std::path::Path;
use std::fs::File;
use std::io::Write;
use std::collections::{HashSet, HashMap};

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde;

#[macro_use]
pub mod include_path;
#[macro_use]
pub mod log;
pub mod common;
pub mod ic_loader;
pub mod logic_formula;
pub mod site_brute_router;

use crate::ic_loader::OpenOpts;
use crate::site_brute_router::{
    BruteRouter,
    PinPairRoutingInfo
};
use crate::log::*;
use crate::common::*;

#[derive(Parser, Debug)]
#[clap(
    author = "Antmicro",
    version = "0.0.1",
    about = "NISP - Nextpnr-fpga_Interchange Site Preprocessor",
    long_about = None
)]
struct Args {
    #[clap(help = "fpga-interchange device file")]
    device: String,
    #[clap(help = "BBA output file")]
    bba: String,
    #[clap(long, help = "Use raw (uncompressed) device file")]
    raw: bool,
    #[clap(long, help = "Tile types to be routed")]
    tile_types: Option<Vec<String>>,
    #[clap(
        long,
        default_value = "1",
        help = "Number of threads to be used during preprocessing"
    )]
    threads: usize,
    #[clap(
        long,
        help = "Tile types to have their routing graphs exported to graphviz .dot files"
    )]
    dot: Option<Vec<String>>,
    #[clap(long, default_value = "", help = "Directory for saving .dot files")]
    dot_prefix: String,
    #[clap(
        long,
        help = "Tile types to have their routing cache exported to JSON format"
    )]
    json: Option<Vec<String>>,
    #[clap(long, default_value = "", help = " Directory for saving .json files")]
    json_prefix: String,
    #[clap(long, help = "Do not optimize logic formulas for constraints")]
    no_formula_opt: bool,
}

struct Exporter {
    prefix: String,
    suffix: String,
    export: HashSet<String>,
    export_all: bool
}

impl Exporter {
    fn new(arg_list: &Option<Vec<String>>, prefix: String, suffix: String) -> Self {
        println!("{:?}", arg_list);
        let mut export_all = false;
        let mut export = HashSet::new();
        if let Some(args) = arg_list {
            for arg in args {
                if arg == ":all" {
                    export_all = true;
                } else {
                    export.insert(arg.clone());
                }
            }
        }

        Self { prefix, suffix, export, export_all }
    }

    fn ignore_or_export_str<F>(&self, name: &str, exporter: F)
        -> std::io::Result<()>
    where
        F: FnOnce() -> String
    {
        if self.export_all || self.export.contains(name) {
            let data = exporter();
            let path = Path::new(&self.prefix)
                .join(Path::new(&(name.to_string() + &self.suffix)));
            let mut file = File::create(path).unwrap();
            return file.write(data.as_bytes()).map(|_| ());
        }
        Ok(())
    }
}

fn map_routing_map_to_serializable<'h>(
    routing_map: &'h HashMap<(usize, usize), PinPairRoutingInfo>)
    -> HashMap<String, &'h PinPairRoutingInfo>
{
    routing_map.iter()
        .map(|(k, v)| (format!("{}->{}", k.0, k.1), v))
        .collect()    
}

impl Serialize for site_brute_router::RoutingInfo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where
        S: serde::Serializer
    {
        use serde::ser::SerializeStruct;

        let mut s = serializer.serialize_struct("RoutingInfo", 3)?;
        
        let serializable_map = map_routing_map_to_serializable(&self.pin_to_pin_routing);
        
        s.serialize_field("pin_to_pin_routing", &serializable_map)?;
        s.serialize_field("out_of_site_sources", &self.out_of_site_sources)?;
        s.serialize_field("out_of_site_sinks", &self.out_of_site_sinks)?;
        s.end()
    }
}

fn main() {
    let args = Args::parse();
    assert!(args.threads != 0);

    let archdef_msg = ic_loader::open(
        Path::new(&args.device), 
        OpenOpts { raw: args.raw }
    ).expect("Couldn't open device file");
    
    let device = archdef_msg.get_archdef_root()
        .expect("Device file does not contain a valid root structure");

    let tile_types: Vec<_> = device.reborrow().get_tile_type_list().unwrap()
        .into_iter()
        .filter(|tt| {
            match &args.tile_types {
                Some(accepted_tile_types) => {
                    accepted_tile_types.iter()
                        .find(|tt_name| {
                            *tt_name == device.ic_str(tt.get_name()).unwrap()
                        })
                        .is_some()
                },
                None => true,
            }
        })
        .collect();
    
    let dot_exporter = Exporter::new(&args.dot, args.dot_prefix.clone(), ".dot".into());
    let json_exporter = Exporter::new(&args.json, args.json_prefix.clone(), ".json".into());
    
    for tt in tile_types {
        let tile_name = device.ic_str(tt.get_name()).unwrap();
        dbg_log!(DBG_INFO, "Processing tile {}", tile_name);
        let brouter = BruteRouter::new(&device, &tt);

        dot_exporter.ignore_or_export_str(&tile_name, || {
            brouter.export_dot(&device, &tile_name)
        }).unwrap();

        let routing_info = if args.threads == 1 {
            brouter.route_all(!args.no_formula_opt)
        } else {
            brouter.route_all_multithreaded(args.threads, !args.no_formula_opt)
        };

        println!(concat!(
            "Tile {}:\n",
            "    No. of intra-site routing pairs:               {}\n",
            "    No. of pins connected to out-of-site-sources:  {}\n",
            "    No. of pins connected to out-of-site-sinks:    {}"
            ),
            tile_name,
            routing_info.pin_to_pin_routing.len(),
            routing_info.out_of_site_sources.len(),
            routing_info.out_of_site_sinks.len()
        );

        json_exporter.ignore_or_export_str(&tile_name, || {
            serde_json::to_string_pretty(&routing_info).unwrap()
        }).unwrap();
    }
}
