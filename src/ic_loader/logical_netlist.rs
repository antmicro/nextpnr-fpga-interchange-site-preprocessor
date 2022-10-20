use capnp;

use super::*;

pub type LogicalNetlistBuilder = capnp::message::TypedBuilder<
    LogicalNetlist_capnp::netlist::Owned
>;
pub type Root<'a> = LogicalNetlist_capnp::netlist::Reader<'a>;
