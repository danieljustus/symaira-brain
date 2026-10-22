//! JSON value tree shared by the Go-parity decoder and the editing document.

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Object(Object),
    Array(Vec<Self>),
    String(String),
    Number(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Object {
    pub keys: Vec<String>,
    pub values: std::collections::HashMap<String, Value>,
}

impl Object {
    pub fn new() -> Self {
        Self {
            keys: Vec::new(),
            values: std::collections::HashMap::new(),
        }
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }
    pub fn set(&mut self, key: &str, value: Value) {
        if !self.values.contains_key(key) {
            self.keys.push(key.to_owned());
        }
        self.values.insert(key.to_owned(), value);
    }
    pub fn remove(&mut self, key: &str) -> bool {
        if self.values.remove(key).is_none() {
            return false;
        }
        self.keys.retain(|item| item != key);
        true
    }
}

impl Drop for Value {
    /// Dismantles nested containers iteratively: Go's decoder accepts
    /// arbitrarily deep documents, so the default recursive drop could
    /// overflow the native stack.
    fn drop(&mut self) {
        let mut pending = Vec::new();
        take_children(self, &mut pending);
        while let Some(mut value) = pending.pop() {
            take_children(&mut value, &mut pending);
        }
    }
}

fn take_children(value: &mut Value, pending: &mut Vec<Value>) {
    match value {
        Value::Array(values) => pending.append(values),
        Value::Object(object) => {
            object.keys.clear();
            pending.extend(object.values.drain().map(|(_, child)| child));
        }
        _ => {}
    }
}
