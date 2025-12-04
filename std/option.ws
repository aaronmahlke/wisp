// Wisp Standard Library - Option Type

import std.io.panic

/// Option represents an optional value: either Some(value) or None.
/// Use this for values that may or may not exist.
pub enum Option<T> {
    Some(T),
    None
}

impl<T> Option<T> {
    /// Returns true if the option contains a value.
    fn is_some(&self) -> bool {
        match *self {
            Some(_) -> true,
            None -> false,
        }
    }
    
    /// Returns true if the option is None.
    fn is_none(&self) -> bool {
        match *self {
            Some(_) -> false,
            None -> true,
        }
    }
    
    /// Returns the contained value or a default.
    /// This is the safe way to extract a value.
    fn or(self, default: T) -> T {
        match self {
            Some(v) -> v,
            None -> default,
        }
    }
    
    /// Forces extraction of the value, panicking if None.
    /// Use this only when you're certain the value exists.
    fn force(self) -> T {
        match self {
            Some(v) -> v,
            None -> panic("called force() on None"),
        }
    }
    
    // TODO: These methods require function type syntax support
    // fn or_else(self, f: () -> T) -> T
    // fn map<U>(self, f: (T) -> U) -> Option<U>
    // fn and_then<U>(self, f: (T) -> Option<U>) -> Option<U>
}
