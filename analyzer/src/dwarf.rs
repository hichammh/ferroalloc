use addr2line::Loader;
use std::sync::Mutex;

// Loader is not Send in addr2line 0.24, so we wrap it in a Mutex to satisfy Arc<Resolver>
// usage across tokio tasks.
pub struct Resolver {
    loader: Mutex<Loader>,
}

impl Resolver {
    /// Load the DWARF debug info from the given ELF/DWARF binary.
    pub fn new(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let loader = Loader::new(path).map_err(|e| format!("addr2line: {e}"))?;
        Ok(Self {
            loader: Mutex::new(loader),
        })
    }

    /// Resolve a raw instruction pointer to its source location.
    /// Returns `None` if the address cannot be mapped (e.g. stdlib frames).
    pub fn resolve(&self, ip: u64) -> Option<SourceLocation> {
        let loader = self.loader.lock().unwrap();
        let mut frames = loader.find_frames(ip).ok()?;
        let frame = frames.next().ok()??;

        let func = frame
            .function
            .and_then(|f| f.demangle().ok().map(|s| s.into_owned()));

        Some(SourceLocation {
            file: frame
                .location
                .as_ref()
                .and_then(|l| l.file.map(String::from)),
            line: frame.location.as_ref().and_then(|l| l.line),
            function: func,
        })
    }
}

// Safety: we guard the non-Send Loader behind a Mutex.
unsafe impl Send for Resolver {}
unsafe impl Sync for Resolver {}

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub function: Option<String>,
}
