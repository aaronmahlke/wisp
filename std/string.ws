// Wisp Standard Library - String type (heap-allocated)

// Memory allocation functions (internal - not exported)
extern fn malloc(size: i64) -> i64
extern fn realloc(ptr: i64, size: i64) -> i64
extern fn free(ptr: i64)
extern fn memcpy(dest: i64, src: i64, n: i64) -> i64
extern fn memset(dest: i64, c: i32, n: i64) -> i64
extern fn strlen(s: i64) -> i64

// Heap-allocated, growable string
pub struct String {
    ptr: i64,   // pointer to char buffer
    len: i64,   // current length (not including null terminator)
    cap: i64,   // capacity (including space for null terminator)
}

impl String {
    // Create an empty string
    pub fn new() -> String {
        let cap: i64 = 16;  // Initial capacity
        let ptr = malloc(cap);
        // Null terminate the empty string
        let one: i64 = 1;
        let _ = memset(ptr, 0, one);
        let zero: i64 = 0;
        String { ptr: ptr, len: zero, cap: cap }
    }
    
    // Create a string from a string literal
    pub fn from(s: str) -> String {
        // str is already a pointer (i64)
        let s_ptr = s as i64;
        let slen = strlen(s_ptr);
        let one: i64 = 1;
        let cap = slen + one;  // +1 for null terminator
        let ptr = malloc(cap);
        let _ = memcpy(ptr, s_ptr, slen + one);  // copy including null
        String { ptr: ptr, len: slen, cap: cap }
    }
    
    // Get the length of the string
    pub fn len(&self) -> i64 {
        self.len
    }
    
    // Check if the string is empty
    pub fn is_empty(&self) -> bool {
        let zero: i64 = 0;
        self.len == zero
    }
    
    // Ensure capacity for at least `min_cap` bytes
    pub fn reserve(&mut self, min_cap: i64) {
        if min_cap > self.cap {
            // Double capacity or use min_cap, whichever is larger
            let two: i64 = 2;
            let mut new_cap = self.cap * two;
            if new_cap < min_cap {
                new_cap = min_cap;
            }
            self.ptr = realloc(self.ptr, new_cap);
            self.cap = new_cap;
        }
    }
    
    // Append a string literal
    pub fn push_str(&mut self, s: str) {
        let s_ptr = s as i64;
        let slen = strlen(s_ptr);
        let new_len = self.len + slen;
        let one: i64 = 1;
        self.reserve(new_len + one);  // +1 for null terminator
        
        // Copy the string data (including null terminator)
        let dest = self.ptr + self.len;
        let _ = memcpy(dest, s_ptr, slen + one);
        self.len = new_len;
    }
    
    // Append another String
    pub fn push_string(&mut self, s: &String) {
        let new_len = self.len + s.len;
        let one: i64 = 1;
        self.reserve(new_len + one);
        
        let dest = self.ptr + self.len;
        let _ = memcpy(dest, s.ptr, s.len);
        // Null terminate
        let _ = memset(self.ptr + new_len, 0, one);
        self.len = new_len;
    }
    
    // Concatenate with a string literal and return a new String
    pub fn concat(&self, s: str) -> String {
        let s_ptr = s as i64;
        let slen = strlen(s_ptr);
        let new_len = self.len + slen;
        let one: i64 = 1;
        let cap = new_len + one;
        let ptr = malloc(cap);
        
        // Copy self's data
        let _ = memcpy(ptr, self.ptr, self.len);
        // Copy s's data (including null terminator)
        let _ = memcpy(ptr + self.len, s_ptr, slen + one);
        
        String { ptr: ptr, len: new_len, cap: cap }
    }
    
    // Concatenate with another String and return a new String
    pub fn concat_string(&self, s: &String) -> String {
        let new_len = self.len + s.len;
        let one: i64 = 1;
        let cap = new_len + one;
        let ptr = malloc(cap);
        
        // Copy self's data
        let _ = memcpy(ptr, self.ptr, self.len);
        // Copy s's data
        let _ = memcpy(ptr + self.len, s.ptr, s.len);
        // Null terminate
        let _ = memset(ptr + new_len, 0, one);
        
        String { ptr: ptr, len: new_len, cap: cap }
    }
    
    // Free the string's memory
    pub fn drop(&mut self) {
        free(self.ptr);
        let zero: i64 = 0;
        self.ptr = zero;
        self.len = zero;
        self.cap = zero;
    }
    
    // Get the raw pointer (for printing)
    pub fn as_ptr(&self) -> i64 {
        self.ptr
    }
    
    // Create a copy of this string
    pub fn clone(&self) -> String {
        let one: i64 = 1;
        let cap = self.len + one;
        let ptr = malloc(cap);
        let _ = memcpy(ptr, self.ptr, self.len + one);
        String { ptr: ptr, len: self.len, cap: cap }
    }
}

// Import ops for Add trait
import std.ops.Add

// Implement Add for String concatenation
impl Add for String {
    fn add(self, rhs: String) -> String {
        let new_len = self.len + rhs.len;
        let one: i64 = 1;
        let cap = new_len + one;
        let ptr = malloc(cap);
        
        // Copy self's data
        let _ = memcpy(ptr, self.ptr, self.len);
        // Copy rhs's data
        let _ = memcpy(ptr + self.len, rhs.ptr, rhs.len);
        // Null terminate
        let _ = memset(ptr + new_len, 0, one);
        
        String { ptr: ptr, len: new_len, cap: cap }
    }
}
