struct Foo<T> {
    x: T
}

impl<T> Foo<T> {
    fn get(self) -> T {
        self.x
    }
}

fn main() {
    let f = Foo { x: 42 }
}
