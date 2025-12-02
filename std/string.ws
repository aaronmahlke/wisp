// Wisp Standard Library - String type
// A heap-allocated, growable string

// C memory functions
extern fn malloc(size: i64) -> i64
extern fn realloc(ptr: i64, size: i64) -> i64
extern fn free(ptr: i64)
extern fn memcpy(dest: i64, src: str, n: i64) -> i64
extern fn strlen(s: str) -> i64

// String struct - heap-allocated, growable
struct String {
    ptr: i64,    // pointer to heap data (null-terminated)
    len: i64,    // length in bytes (not including null terminator)
    cap: i64,    // capacity in bytes (including null terminator)
}

impl String {
    // Create a string from a string literal
    fn from(s: str) -> String {
        let len = strlen(s);
        let cap = len + 1;
        let ptr = malloc(cap);
        let _ = memcpy(ptr, s, cap);  // Copy including null terminator
        String { ptr: ptr, len: len, cap: cap }
    }
    
    // Get the length of the string
    fn len(&self) -> i64 {
        self.len
    }
    
    // Check if empty
    fn is_empty(&self) -> bool {
        self.len == 0
    }
    
    // Get as raw pointer (for C interop)
    fn as_ptr(&self) -> i64 {
        self.ptr
    }
    
    // Free the string's memory
    fn drop(&self) {
        if self.ptr != 0 {
            free(self.ptr);
        }
    }
}

