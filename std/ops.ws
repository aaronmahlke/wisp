// Wisp Standard Library - Operator Traits

// Addition operator trait
// Usage: impl Add for MyType { fn add(self, rhs: Self) -> Self { ... } }
// Or:    impl Add<OtherType> for MyType { fn add(self, rhs: OtherType) -> Self { ... } }
trait Add<Rhs = Self> {
    fn add(self, rhs: Rhs) -> Self
}

// Subtraction operator trait
trait Sub<Rhs = Self> {
    fn sub(self, rhs: Rhs) -> Self
}

// Multiplication operator trait
trait Mul<Rhs = Self> {
    fn mul(self, rhs: Rhs) -> Self
}

// Division operator trait
trait Div<Rhs = Self> {
    fn div(self, rhs: Rhs) -> Self
}

// Remainder/Modulo operator trait
trait Rem<Rhs = Self> {
    fn rem(self, rhs: Rhs) -> Self
}

// Negation operator trait (unary -)
trait Neg {
    fn neg(self) -> Self
}

// Bitwise AND operator trait
trait BitAnd<Rhs = Self> {
    fn bitand(self, rhs: Rhs) -> Self
}

// Bitwise OR operator trait
trait BitOr<Rhs = Self> {
    fn bitor(self, rhs: Rhs) -> Self
}

// Bitwise XOR operator trait
trait BitXor<Rhs = Self> {
    fn bitxor(self, rhs: Rhs) -> Self
}

// Bitwise NOT operator trait (unary ~)
trait Not {
    fn not(self) -> Self
}

// Left shift operator trait
trait Shl<Rhs = Self> {
    fn shl(self, rhs: Rhs) -> Self
}

// Right shift operator trait
trait Shr<Rhs = Self> {
    fn shr(self, rhs: Rhs) -> Self
}

