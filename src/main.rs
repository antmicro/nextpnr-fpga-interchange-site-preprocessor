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

use clap::{arg, Parser};
use lazy_static::__Deref;
use crate::router::site_brute_router::PinId;
use serde::Serialize;
use std::path::Path;
use std::fs::File;
use std::io::Write;
use std::collections::{HashSet, HashMap};
use std::sync::{Arc, Mutex};

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
pub mod router;
pub mod dot_exporter;

use crate::ic_loader::OpenOpts;
use crate::router::site_brute_router::{
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
    #[command(subcommand)]
    command: SubCommands,
}

#[derive(Parser, Debug)]
struct PreprocessCmd {
    #[arg(long, help = "Tile types to be routed")]
    tile_types: Option<Vec<String>>,
    #[arg(
        long,
        default_value = "1",
        help = "Number of threads to be used during preprocessing"
    )]
    threads: usize,
    #[arg(
        long,
        help = "Tile types to have their routing graphs exported to graphviz .dot files"
    )]
    dot: Option<Vec<String>>,
    #[arg(long, default_value = "", help = "Directory for saving .dot files")]
    dot_prefix: String,
    #[arg(
        long,
        help = "Tile types to have their routing cache exported to JSON format"
    )]
    json: Option<Vec<String>>,
    #[arg(long, default_value = "", help = " Directory for saving .json files")]
    json_prefix: String,
    #[arg(long, help = "Do not optimize logic formulas for constraints")]
    no_formula_opt: bool,
}

#[derive(Parser, Debug)]
struct RoutePairCmd {
    #[arg(help = "Tile Type")]
    tile_type: String,
    #[arg(help = "Path to source pin: site_name/bel_name.pin_name")]
    from: String,
    #[arg(help = "Path to destination pin: site_name/bel_name.pin_name")]
    to: String,
}

impl RoutePairCmd {
    fn get_from_triple<'s>(&'s self) -> Result<(&'s str, &'s str, &'s str), ()> {
        let (site_name, tail) = self.from.split_once('/').ok_or(())?;
        let (bel_name, pin_name) = tail.split_once('.').ok_or(())?;

        Ok((site_name, bel_name, pin_name))
    }

    fn get_to_triple<'s>(&'s self) -> Result<(&'s str, &'s str, &'s str), ()> {
        let (site_name, tail) = self.to.split_once('/').ok_or(())?;
        let (bel_name, pin_name) = tail.split_once('.').ok_or(())?;

        Ok((site_name, bel_name, pin_name))
    }
}

#[derive(Parser, Debug)]
enum SubCommands {
    Preprocess(PreprocessCmd),
    RoutePair(RoutePairCmd),
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

impl Serialize for router::site_brute_router::RoutingInfo {
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

fn preprocess<'d>(args: PreprocessCmd, device: ic_loader::archdef::Root<'d>) {
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
    
    for (tt_id, tt) in tile_types.iter().enumerate() {
        let tile_name = device.ic_str(tt.get_name()).unwrap();
        dbg_log!(DBG_INFO, "Processing tile {}", tile_name);
        let brouter = BruteRouter::<()>::new(&device, tt_id as u32);

        dot_exporter.ignore_or_export_str(&tile_name, || {
            brouter.create_dot_exporter(&device).export_dot(&device, &tile_name)
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

fn route_pair<'d>(args: RoutePairCmd, device: ic_loader::archdef::Root<'d>) {
    let (tt_id, _) = device.reborrow().get_tile_type_list().unwrap()
        .into_iter()
        .enumerate()
        .find(|(_, tt)| {
            device.ic_str(tt.get_name()).unwrap() == args.tile_type
        })
        .expect("Wrong tile type name");
    
    let (from_site, from_bel, from_pin) = args.get_from_triple()
        .expect("Incorrent from pin format!");
    let (to_site, to_bel, to_pin) = args.get_to_triple()
        .expect("Incorrent to pin format!");

    
    let router_state = Arc::new(Mutex::new(HashMap::new()));
    //let rs = Arc::clone(&router_state);
    let routes = Arc::new(Mutex::new(Vec::new()));
    let routes_l = Arc::clone(&routes);

    let brouter = BruteRouter::<Vec<PinId>>::new(&device, tt_id as u32);
    
    let from = brouter.get_pin_id(&device, from_site, from_bel, from_pin)
        .expect("From pin does not exist!");
    
    let to = brouter.get_pin_id(&device, to_site, to_bel, to_pin)
        .expect("To pin does not exist!");
    
    let brouter = brouter.with_callback(move |frame| {
        let mut rs = router_state.deref().lock().unwrap();

        rs.insert(frame.node, frame.prev_node);

        let mut acc = frame.accumulator.clone();
        acc.push(frame.node);

        /* Save newly found route */
        if frame.node == to {
            routes.deref().lock().unwrap().push(acc.clone());
        }

        (None, None, acc)
    });

    let _ = brouter.route_pins(from, None, false).enumerate();

    println!("Explored the following routes:");
    for (route_id, route) in routes_l.deref().lock().unwrap().deref().iter().enumerate() {
        println!("  Route #{}:", route_id);
        for pin in route {
            let (site, bel, pin) = brouter.get_pin_name(&device, *pin);
            println!("    {}/{}.{}", site, bel, pin);
        }
    }

   /*  println!("Visited nodes: {:?}", rs.deref().lock().unwrap()); */
}

fn main() {
    let args = Args::parse();

    if let SubCommands::Preprocess(prepreocess) = &args.command {
        assert!(prepreocess.threads != 0);
    }

    let archdef_msg = ic_loader::open(
        Path::new(&args.device), 
        OpenOpts { raw: args.raw }
    ).expect("Couldn't open device file");
    
    let device = archdef_msg.get_archdef_root()
        .expect("Device file does not contain a valid root structure");
    
    match args.command {
        SubCommands::Preprocess(sargs) => preprocess(sargs, device),
        SubCommands::RoutePair(sargs) => route_pair(sargs, device),
    }
}
