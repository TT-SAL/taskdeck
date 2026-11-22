//! The bridge envelope (bridge.md §5.2, §5.3, §21).

/// A `req` envelope from the page (bridge.md §5.2).
pub struct Request {
    pub id: u64,
    pub method: Method,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Option<String>,
    pub body_b64: Option<String>,
    pub request_key: Option<String>,
    pub clear: bool,
}

/// A request's method (bridge.md §5.2).
pub enum Method {
    Get,
    Post,
}

/// A reply to the page (bridge.md §5.3).
pub struct Reply {
    pub status: u16,
    pub body: Option<serde_json::Value>,
}

/// Parses a `req` envelope; `Err` is the 400 (bridge.md §5.2 steps 3 and 4).
pub fn parse_request(text: &str) -> Result<Request, Reply> {
    unimplemented!()
}

/// The JSON text of `request`'s result (bridge.md §11.2).
pub fn reply_json(reply: &Reply) -> String {
    unimplemented!()
}

/// The decoded length of a `body_b64` from its character count (bridge.md §5.7, BRIDGE-MSG-011).
pub fn decoded_len(body_b64: &str) -> Option<usize> {
    unimplemented!()
}
