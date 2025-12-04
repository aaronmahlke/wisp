import std.string
import std.ops

extern fn system(command: str) -> i32

fn open_application(path: str) -> i32 {
    let open_cmd = String.from("open '")
    let path_str = String.from(path)
    let close_quote = String.from("'")
    let command = open_cmd + path_str + close_quote
    system(command.as_ptr() as str)
}

fn main() -> i32 {
    let app_path = "/Applications/DaVinci Resolve/DaVinci Resolve.app"
    open_application(app_path)
}
