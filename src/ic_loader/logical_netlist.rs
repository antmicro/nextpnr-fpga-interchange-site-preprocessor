use capnp;
use std::io::{BufReader, BufWriter};
use flate2;

use super::*;

pub type LogicalNetlistBuilder = capnp::message::TypedBuilder<
    LogicalNetlist_capnp::netlist::Owned
>;
pub type Root<'a> = LogicalNetlist_capnp::netlist::Reader<'a>;
