//! # Interaction combinator runtime
//!
//! Par ships with a runtime based on Interaction Combinators. Interaction Combinators are a model of computation
//! based on graph-rewriting. They are a specific case of interaction nets.
//!
//! In an interaction net, each node has a principal port and an amount of auxiliary ports. Nodes interact by their principal port,
//! according to some pre-defined set of interaction rules
//!
//! The [`net`] module contains the trees and nets, and their interaction rules.
//!
//! These nets are the compile-time representation of Par programs: `backend::tree::compiler`
//! compiles checked processes into packages of trees, normalizing each package at compile time,
//! and `backend::flat::transpiler` turns them into the flat representation that `par-runtime`
//! runs.

pub(crate) mod net;
pub(crate) use net::Net;
