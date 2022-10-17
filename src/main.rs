use clap::Parser;
use std::path::Path;
use std::fs::File;
use std::io::Write;

#[macro_use]
extern crate lazy_static;

#[macro_use]
pub mod include_path;
#[macro_use]
pub mod log;
pub mod ic_loader;
pub mod logic_formula;
pub mod site_brute_router;

use crate::ic_loader::OpenOpts;
use crate::site_brute_router::BruteRouter;
use crate::log::*;

#[derive(Parser, Debug)]
#[clap(
    author = "Antmicro",
    version = "0.0.1",
    about = "NISP - Nextpnr-fpga_Interchange Site Preprocessor",
    long_about = None
)]
struct Args {
    device: String,
    bba: String,
    #[clap(long)]
    raw: bool,
    #[clap(short, long, default_value = "6")]
    compression_level: u32,
    #[clap(long)]
    tile_types: Option<Vec<String>>,
    #[clap(long, default_value = "1")]
    threads: usize,
    #[clap(long)]
    dot: Option<Vec<String>>,
    #[clap(long, default_value = "")]
    dot_prefix: String,
}

pub trait IcStr<'a> {
    fn ic_str(&self, id: u32) -> Result<&'a str, capnp::Error>;
}

impl<'a> IcStr<'a> for ic_loader::archdef::Root<'a> {
    fn ic_str(&self, id: u32) -> Result<&'a str, capnp::Error> {
        self.get_str_list().unwrap().get(id)
    }
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
    
    let mut export_all_dots = false;
    let mut export_dots = std::collections::HashSet::new();
    if let Some(dots) = args.dot {
        for dot in &dots {
            if dot == ":all" {
                export_all_dots = true;
            } else {
                export_dots.insert(dot.clone());
            }
        }
    }
    
    for tt in tile_types {
        let tile_name = inputs.device.ic_str(tt.get_name()).unwrap();
        dbg_log!(DBG_INFO, "Processing tile {}", tile_name);
        let brouter = BruteRouter::new(&inputs, &tt);

        if export_all_dots || export_dots.contains(tile_name) {
            export_dot(&inputs, &args.dot_prefix, &tile_name, &brouter).unwrap();
        }

        let routing_map = if args.threads == 1 {
            brouter.route_all()
        } else {
            brouter.route_all_multithreaded(args.threads)
        };
        println!("No. of routing pairs for tile {}: {}", tile_name, routing_map.len());
    }
}

fn export_dot<'a>(
    inputs: &Inputs<'a>,
    dot_prefix: &str,
    tile_name: &str,
    router: &BruteRouter<'a>
) -> std::io::Result<usize> {

    let dot = router.export_dot(&inputs, tile_name);
    
    let path = Path::new(dot_prefix).join(Path::new(&(tile_name.to_string() + ".dot")));
    let mut file = File::create(path).unwrap();
    file.write(dot.as_bytes())
}
