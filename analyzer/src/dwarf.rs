use std::path::Path;
use addr2line::{Context, Location};

pub struct Resolver {
    ctx: Context<gimli::EndianRcSlice<gimli::RunTimeEndian>>,
}

impl Resolver {
    /// Load the DWARF debug info from the given ELF/DWARF binary.
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let data = std::fs::read(Path::new(path))?;
        let object = object::File::parse(data.as_slice())?;
        let ctx = Context::new(&object)?;
        Ok(Self { ctx })
    }

    /// Resolve a raw instruction pointer to its source location.
    /// Returns `None` if the address cannot be mapped (e.g. stdlib frames).
    pub fn resolve(&self, ip: u64) -> Option<SourceLocation> {
        let mut frames = self.ctx.find_frames(ip).ok()?;
        let frame = frames.next().ok()??;

        let loc: Option<Location> = frame.location;
        let func = frame.function
            .and_then(|f| f.demangle().ok().map(|s| s.into_owned()));

        Some(SourceLocation {
            file: loc.as_ref().and_then(|l| l.file.map(|s| s.to_string())),
            line: loc.as_ref().and_then(|l| l.line),
            function: func,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub function: Option<String>,
}
