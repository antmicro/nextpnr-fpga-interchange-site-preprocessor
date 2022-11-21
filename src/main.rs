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
use crate::router::TilePinId;
use std::path::Path;
use std::collections::HashMap;
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
pub mod exporter;
pub mod dot_exporter;

use crate::ic_loader::OpenOpts;
use crate::router::site_brute_router::BruteRouter;
use crate::exporter::Exporter;
use crate::router::serialize::*;
#[allow(unused)]
use crate::log::*;
use crate::common::*;
use crate::exporter::*;

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
    #[arg(long, help = "Add debugging hints to the exported JSON")]
    with_debug_hints: bool,
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
    
    let mut dot_exporter =
        MultiFileExporter::new(&args.dot, args.dot_prefix.clone(), ".dot".into());
    
    /* Unfortunately, since serde::Serialize is not object-safe, we need separate
     * exporters for different types. */
    let mut json_exporter = CompoundJsonExporter::new(
        &args.json,
        Path::new(&args.json_prefix).join(
            format!("{}_site_routability.json", device.get_name().unwrap())
        )
    );
    let mut debug_json_exporter = CompoundJsonExporter::new(
        &args.json,
        Path::new(&args.json_prefix).join(
            format!("{}_site_routability.json", device.get_name().unwrap())
        )
    );
    
    for (tt_id, tt) in tile_types.iter().enumerate() {
        let tile_name = device.ic_str(tt.get_name()).unwrap();
        dbg_log!(DBG_INFO, "Processing tile {}", tile_name);
        let brouter = BruteRouter::<()>::new(&device, tt_id as u32);

        dot_exporter.ignore_or_export(&tile_name, || {
            brouter.create_dot_exporter(&device).export_dot(&device, &tile_name)
        }).unwrap();

        let brouter = Arc::new(brouter);
        let routing_info = if args.threads == 1 {
            brouter.as_ref().route_all(!args.no_formula_opt)
        } else {
            use crate::router::site_brute_router::MultiThreadedBruteRouter;
            Arc::clone(&brouter)
                .route_all_multithreaded(args.threads, !args.no_formula_opt)
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

        if args.with_debug_hints {
            debug_json_exporter.ignore_or_export(&tile_name, ||
                Box::new(routing_info.with_debug_info(brouter, &device))
            ).unwrap();
        } else {
            json_exporter.ignore_or_export(&tile_name, || Box::new(routing_info))
                .unwrap();
        }
    }
    
    <MultiFileExporter as Exporter<String>>::flush(&mut dot_exporter).unwrap();

    if args.with_debug_hints {
        debug_json_exporter.flush().unwrap();
    } else {
        json_exporter.flush().unwrap();
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

    let brouter = BruteRouter::<Vec<TilePinId>>::new(&device, tt_id as u32);
    
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
            println!("{}", brouter.get_pin_name(&device, *pin).to_string());
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
