// Test file for borrow checking

struct Point {
    x: i32,
    y: i32,
}

// === Test 1: Basic ownership ===
fn test_ownership() {
    let x = 5;
    let y = x;  // Copy (i32 is Copy)
    let z = x;  // Still valid because i32 is Copy
}

// === Test 2: Mutable references ===
fn test_mut_ref() {
    let mut x = 5;
    let r = &mut x;
    *r = 10;
    // x is now 10
}

// === Test 3: Immutable references ===
fn test_immut_ref() {
    let x = 5;
    let r1 = &x;
    let r2 = &x;  // Multiple immutable borrows OK
}

// === Test 4: Swap through references ===
fn swap(a: &mut i32, b: &mut i32) {
    let tmp = *a;
    *a = *b;
    *b = tmp;
}

// === Test 5: Struct creation ===
fn test_struct() {
    let p = Point { x: 1, y: 2 };
    let px = p.x;  // Field access
}

fn main() {
    test_ownership();
    test_mut_ref();
    test_immut_ref();
    test_struct();
}

