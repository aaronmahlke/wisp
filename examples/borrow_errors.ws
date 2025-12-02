// Test file with intentional borrow errors

struct Point {
    x: i32,
    y: i32,
}

// === Error 1: Use after move ===
fn use_after_move() {
    let p = Point { x: 1, y: 2 };
    let q = p;   // p is moved to q
    let r = p;   // ERROR: use of moved value p
}

// === Error 2: Mutable borrow while borrowed ===
fn double_borrow() {
    let mut x = 5;
    let r1 = &x;      // immutable borrow
    let r2 = &mut x;  // ERROR: cannot borrow as mutable while borrowed
}

// === Error 3: Use while mutably borrowed ===
fn use_while_borrowed() {
    let mut x = 5;
    let r = &mut x;   // mutable borrow
    let y = x;        // ERROR: cannot use x while mutably borrowed
}

// === Error 4: Assign to immutable ===
fn assign_immutable() {
    let x = 5;
    x = 10;  // ERROR: cannot assign to x, as it is not declared as mutable
}

fn main() {
}

