//! Prints every export of a cooked package with its tagged properties (research aid).
//!
//! `cargo run -p wacca-ue4 --example dump_objects -- <package.uasset|.umap>` (the `.uexp` is read
//! alongside).

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args()
        .nth(1)
        .ok_or("usage: dump_objects <package>")?;
    let uasset = std::fs::read(&arg)?;
    let uexp = std::fs::read(Path::new(&arg).with_extension("uexp"))?;
    let objs = wacca_ue4::read_objects(&uasset, &uexp)?;
    for i in 1.. {
        let Some(name) = objs.name(-i) else { break };
        println!("import -{i} {name}");
    }
    for (i, o) in objs.exports.iter().enumerate() {
        println!("#{} {} ({}) outer {}", i + 1, o.name, o.class, o.outer);
        for (n, v) in &o.properties {
            println!("    {n} = {v:?}");
        }
    }
    Ok(())
}
