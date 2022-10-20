use clap::Parser;
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
use crate::site_brute_router::{BruteRouter, RoutingInfo};
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


pub struct Inputs<'a> {
    pub device: ic_loader::archdef::Root<'a>,
    /* pub netlist: ic_loader::logical_netlist::Root<'a>, */
}

impl<'a> Inputs<'a> {
    fn new(archdef: &'a Box<dyn ic_loader::MsgReader>/* , lnet: &'a Box<dyn ic_loader::MsgReader> */)
        -> Self
    {
        Self {
            device: archdef.get_archdef_root()
                .expect("Device file does not contain a valid root structure"),
        }
    }
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

fn map_routing_map_to_serializable(routing_map: HashMap<(usize, usize), RoutingInfo>)
    -> HashMap<String, RoutingInfo>
{
    routing_map.into_iter()
        .map(|(k, v)| (format!("{}->{}", k.0, k.1), v))
        .collect()    
}

fn main() {
    let args = Args::parse();
    assert!(args.threads != 0);

    let archdef_msg = ic_loader::open(
        Path::new(&args.device), 
        OpenOpts { raw: args.raw }
    ).expect("Couldn't open device file");
    
    let inputs = Inputs::new(&archdef_msg/* , &lnet_msg */);

    let tile_types: Vec<_> = inputs.device.reborrow().get_tile_type_list().unwrap()
        .into_iter()
        .filter(|tt| {
            match &args.tile_types {
                Some(accepted_tile_types) => {
                    accepted_tile_types.iter()
                        .find(|tt_name| {
                            *tt_name == inputs.device.ic_str(tt.get_name()).unwrap()
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
        let tile_name = inputs.device.ic_str(tt.get_name()).unwrap();
        dbg_log!(DBG_INFO, "Processing tile {}", tile_name);
        let brouter = BruteRouter::new(&inputs.device, &tt);

        dot_exporter.ignore_or_export_str(&tile_name, || {
            brouter.export_dot(&inputs.device, &tile_name)
        }).unwrap();

        let routing_map = if args.threads == 1 {
            brouter.route_all(!args.no_formula_opt)
        } else {
            brouter.route_all_multithreaded(args.threads, !args.no_formula_opt)
        };
        println!("No. of routing pairs for tile {}: {}", tile_name, routing_map.len());

        json_exporter.ignore_or_export_str(&tile_name, || {
            serde_json::to_string_pretty(
                &map_routing_map_to_serializable(routing_map)
            ).unwrap()
        }).unwrap();
    }
}
