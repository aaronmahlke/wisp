import std.option.{ Option, Some, None }
import std.io.print

fn main() {
    let x: Option<i32> = Some(42)
    
    // Call is_some method
    let result = x.is_some()
    
    if result {
        print(&"is Some")
    } else {
        print(&"is None")
    }
}

