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
use crate::router::SitePinId;
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
pub mod strings;
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
use crate::strings::*;

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
    #[arg(long, help = "Site types to be routed")]
    site_types: Option<Vec<String>>,
    #[arg(
        long,
        default_value = "1",
        help = "Number of threads to be used during preprocessing"
    )]
    threads: usize,
    #[arg(
        long,
        help = "Site types to have their routing graphs exported to graphviz .dot files"
    )]
    dot: Option<Vec<String>>,
    #[arg(long, default_value = "", help = "Directory for saving .dot files")]
    dot_prefix: String,
    #[arg(
        long,
        help = "Site types to have their routing cache exported to JSON format"
    )]
    json: Option<Vec<String>>,
    #[arg(long, default_value = "", help = " Directory for saving .json files")]
    json_prefix: String,
    #[arg(long, help = "Do not optimize logic formulas for constraints")]
    no_formula_opt: bool,
    #[arg(
        short = 'c',
        long,
        help = "Add $VCC and $GND ports to sites with constant generators")
    ]
    virtual_consts: bool
}

#[derive(Parser, Debug)]
struct RoutePairCmd {
    #[arg(help = "Site Type")]
    tile_type: String,
    #[arg(help = "Path to source pin: bel_name.pin_name")]
    from: String,
    #[arg(help = "Path to destination pin: bel_name.pin_name")]
    to: String,
}

impl RoutePairCmd {
    fn get_from_tuple<'s>(&'s self) -> Result<(&'s str, &'s str), ()> {
        let (bel_name, pin_name) = self.from.split_once('.').ok_or(())?;

        Ok((bel_name, pin_name))
    }

    fn get_to_tuple<'s>(&'s self) -> Result<(&'s str, &'s str), ()> {
        let (bel_name, pin_name) = self.to.split_once('.').ok_or(())?;

        Ok((bel_name, pin_name))
    }
}

#[derive(Parser, Debug)]
enum SubCommands {
    Preprocess(PreprocessCmd),
    RoutePair(RoutePairCmd),
}

fn preprocess<'d>(args: PreprocessCmd, device: ic_loader::archdef::Root<'d>) {
    let site_types: Vec<_> = device.get_site_type_list().unwrap()
        .into_iter()
        .enumerate()
        .filter(|(_, tt)| {
            match &args.site_types {
                Some(accepted_site_types) => {
                    accepted_site_types.iter()
                        .find(|st_name| {
                            *st_name == device.ic_str(tt.get_name())
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

    for (st_id, st) in site_types {
        let st_name = device.ic_str(st.get_name());
        dbg_log!(DBG_INFO, "Processing site type {}", st_name);
        let brouter = BruteRouter::<()>::new(&device, st_id as u32, args.virtual_consts);

        dot_exporter.ignore_or_export(&st_name, || {
            brouter.create_dot_exporter().export_dot(&device, &st_name)
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
            "Site Type {}:\n",
            "    No. of intra-site routing pairs:               {}\n",
            "    No. of pins connected to out-of-site-sources:  {}\n",
            "    No. of pins connected to out-of-site-sinks:    {}"
            ),
            st_name,
            routing_info.pin_to_pin_routing.len(),
            routing_info.out_of_site_sources.len(),
            routing_info.out_of_site_sinks.len()
        );

        json_exporter.ignore_or_export(&st_name, ||
            routing_info.with_extras(brouter, &device)
        ).unwrap();
    }
    
    <MultiFileExporter as Exporter<String>>::flush(&mut dot_exporter).unwrap();

    json_exporter.flush().unwrap();
}

fn route_pair<'d>(args: RoutePairCmd, device: ic_loader::archdef::Root<'d>) {
    let (tt_id, _) = device.reborrow().get_tile_type_list().unwrap()
        .into_iter()
        .enumerate()
        .find(|(_, tt)| device.ic_str(tt.get_name()) == args.tile_type)
        .expect("Wrong tile type name");
    
    let (from_bel, from_pin) = args.get_from_tuple()
        .expect("Incorrent from pin format!");
    let (to_bel, to_pin) = args.get_to_tuple()
        .expect("Incorrent to pin format!");
    
    let router_state = Arc::new(Mutex::new(HashMap::new()));
    //let rs = Arc::clone(&router_state);
    let routes = Arc::new(Mutex::new(Vec::new()));
    let routes_l = Arc::clone(&routes);

    let brouter = BruteRouter::<Vec<SitePinId>>::new(&device, tt_id as u32, false);
    
    let from = brouter.get_pin_id(&device, from_bel, from_pin)
        .expect("From pin does not exist!");
    
    let to = brouter.get_pin_id(&device, to_bel, to_pin)
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

    let _ = brouter.route_pins(from, false);

    let gsctx = GlobalStringsCtx::hold();
    println!("Explored the following routes:");
    for (route_id, route) in routes_l.deref().lock().unwrap().deref().iter().enumerate() {
        println!("  Route #{}:", route_id);
        for pin in route {
            println!("    {}", brouter.get_pin_name(&device, &gsctx, *pin).to_string());
        }
    }
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
