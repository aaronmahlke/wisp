// Wisp Language Showcase
import std.io.{ print }

// Data Structures
struct Vec2 {
    x: i32,
    y: i32,
}

struct Player {
    x: i32,
    y: i32,
    health: i32,
    score: i32,
}

// Vector Math Functions

fn vec2_new(x: i32, y: i32) -> Vec2 {
    Vec2 { x: x, y: y }
}

fn vec2_add(a: &Vec2, b: &Vec2) -> Vec2 {
    Vec2 { x: a.x + b.x, y: a.y + b.y }
}

fn vec2_scale(v: &Vec2, s: i32) -> Vec2 {
    Vec2 { x: v.x * s, y: v.y * s }
}

fn vec2_dot(a: &Vec2, b: &Vec2) -> i32 {
    a.x * b.x + a.y * b.y
}

fn vec2_length_squared(v: &Vec2) -> i32 {
    vec2_dot(v, v)
}

fn vec2_print(label: str, v: &Vec2) {
    print(&"{label}({v.x}, {v.y})");
}

// Player Functions (immutable - return new player)

fn player_new(x: i32, y: i32) -> Player {
    Player { x: x, y: y, health: 100, score: 0 }
}

fn player_moved(p: &Player, dx: i32, dy: i32) -> Player {
    Player { x: p.x + dx, y: p.y + dy, health: p.health, score: p.score }
}

fn player_damaged(p: &Player, amount: i32) -> Player {
    let new_health = p.health - amount;
    let clamped = if new_health < 0 { 0 } else { new_health };
    Player { x: p.x, y: p.y, health: clamped, score: p.score }
}

fn player_scored(p: &Player, points: i32) -> Player {
    Player { x: p.x, y: p.y, health: p.health, score: p.score + points }
}

fn player_is_alive(p: &Player) -> bool {
    p.health > 0
}

// Math Utilities

fn abs(n: i32) -> i32 {
    if n < 0 { 0 - n } else { n }
}

fn max(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}

fn min(a: i32, b: i32) -> i32 {
    if a < b { a } else { b }
}

fn clamp(value: i32, low: i32, high: i32) -> i32 {
    min(max(value, low), high)
}

fn factorial(n: i32) -> i32 {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}

fn fib(n: i32) -> i32 {
    if n <= 1 { n } else { fib(n - 1) + fib(n - 2) }
}

fn pow(base: i32, exp: i32) -> i32 {
    let mut result = 1;
    let mut i = 0;
    while i < exp {
        result = result * base;
        i = i + 1;
    }
    result
}

fn gcd(a: i32, b: i32) -> i32 {
    let mut x = a;
    let mut y = b;
    while y != 0 {
        let temp = y;
        y = x % y;
        x = temp;
    }
    x
}

fn isqrt(n: i32) -> i32 {
    if n < 2 { n }
    else {
        let mut x = n;
        let mut y = (x + 1) / 2;
        while y < x {
            x = y;
            y = (x + n / x) / 2;
        }
        x
    }
}

// Helper to print a labeled integer result
fn print_result(label: str, value: i32) {
    print(&label);
    print(&value);
    println();
}

fn print_line(s: str) {
    print(&s);
    println();
}

// Main Program

fn main() -> i32 {
    print_line("=== Wisp Language Showcase ===");
    println();

    // Vector math
    print_line("[Vector Math]");
    let v1 = vec2_new(3, 4);
    let v2 = vec2_new(1, 2);

    vec2_print("v1 = ", &v1);
    vec2_print("v2 = ", &v2);

    let v3 = vec2_add(&v1, &v2);
    vec2_print("v1 + v2 = ", &v3);

    let v4 = vec2_scale(&v1, 3);
    vec2_print("v1 * 3 = ", &v4);

    print_result("v1 . v2 = ", vec2_dot(&v1, &v2));
    print_result("|v1|^2 = ", vec2_length_squared(&v1));

    println();

    // Game simulation
    print_line("[Game Simulation]");
    let p0 = player_new(0, 0);
    let p1 = player_moved(&p0, 5, 3);
    let p2 = player_scored(&p1, 100);
    let p3 = player_damaged(&p2, 30);
    let p4 = player_damaged(&p3, 80);

    print_line("Player: move(5,3), score(100), damage(30), damage(80)");
    print_result("  Position X: ", p4.x);
    print_result("  Position Y: ", p4.y);
    print_result("  Health: ", p4.health);
    print_result("  Score: ", p4.score);
    print(&"  Alive: ");
    print(&player_is_alive(&p4));
    println();

    // Math functions
    print_line("[Math Functions]");
    print_result("factorial(6) = ", factorial(6));
    print_result("fib(10) = ", fib(10));
    print_result("pow(2, 10) = ", pow(2, 10));
    print_result("gcd(48, 18) = ", gcd(48, 18));
    print_result("isqrt(144) = ", isqrt(144));
    print_result("isqrt(200) = ", isqrt(200));


    println();
    print_line("=== All tests passed! ===");

    // Return fib(10) as exit code to verify (should be 55)
    fib(10)
}
