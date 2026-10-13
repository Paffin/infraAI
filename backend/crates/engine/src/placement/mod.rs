//! Placement generator: turns a *pattern* (how many sites, how they are grouped, what is tolerated) and a *stack*
//! (the layers of a platform) into a complete architecture for one site, two, five, ten… The generator decides
//! nothing about correctness: the result goes through the same rule engine as everything else, and the notes say
//! where the generator had to deviate from what was asked.

mod gen;
mod model;

pub use gen::generate;
pub use model::*;
