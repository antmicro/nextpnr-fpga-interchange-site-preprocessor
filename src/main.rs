use clap::Parser;
use std::path::Path;

#[macro_use]
pub mod include_path;
pub mod ic_loader;
pub mod logic_formula;
pub mod site_brute_router;

use crate::ic_loader::OpenOpts;
use crate::site_brute_router::BruteRouter;

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
    tile_types: Option<Vec<String>>
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
    
    for tt in tile_types {
        let _ = BruteRouter::new(&inputs, &tt);
    }
}
