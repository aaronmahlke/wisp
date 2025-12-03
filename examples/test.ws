import std.io.{ print, Display }
import std.string.String

fn add_bumbers(a: i32, b: i32) -> i32 {
    a + b
}

struct TheGoodOne {
    value: i32,
}

struct TwistedTwingly {
    value: i32,
}

fn twisting(little_twist: TheGoodOne) -> TwistedTwingly {
    TwistedTwingly { value: little_twist.value * 2 }
}

impl Display for TwistedTwingly {
    fn to_string(&self) -> String {
        "TwistedTwingly {self.value}"
    }
}

fn main() {
    let mut number = 3;
    number = add_bumbers(number, number);
    let twisted_twingly = twisting(TheGoodOne { value: number });
    print(&twisted_twingly);
}
