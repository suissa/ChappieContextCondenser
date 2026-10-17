use serde_json::Value;

pub struct Encoded {
    pub wire: String,
    pub decoded: Value,
}

pub trait Transform {
    fn id(&self) -> &'static str;
    fn try_encode(&self, value: &Value) -> Option<Encoded>;
    fn trusted(&self) -> bool {
        false
    }
}

// Text codecs prove their reconstruction. The default is byte equality; semantic formats such as
// YAML may override this when comments/formatting are intentionally discarded from the model wire.
pub trait TextProposer: Send + Sync {
    fn id(&self) -> &'static str;
    fn encode(&self, raw: &str) -> Option<String>;
    fn decode(&self, wire: &str) -> Option<String>;
    fn verify(&self, raw: &str, wire: &str) -> bool {
        self.decode(wire).as_deref() == Some(raw)
    }
}

type CodecClosure = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct CallbackProposer {
    encode: CodecClosure,
    decode: CodecClosure,
}

impl CallbackProposer {
    pub fn new(
        encode: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
        decode: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            encode: Box::new(encode),
            decode: Box::new(decode),
        }
    }
}

impl TextProposer for CallbackProposer {
    fn id(&self) -> &'static str {
        "custom"
    }
    fn encode(&self, raw: &str) -> Option<String> {
        (self.encode)(raw)
    }
    fn decode(&self, wire: &str) -> Option<String> {
        (self.decode)(wire)
    }
}
