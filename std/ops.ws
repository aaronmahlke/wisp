// Wisp Standard Library - Operator Traits

// Addition operator trait
// Usage: impl Add for MyType { fn add(self, rhs: Self) -> Self { ... } }
// Or:    impl Add<OtherType> for MyType { fn add(self, rhs: OtherType) -> Self { ... } }
pub trait Add<Rhs = Self> {
    fn add(self, rhs: Rhs) -> Self
}

// Subtraction operator trait
pub trait Sub<Rhs = Self> {
    fn sub(self, rhs: Rhs) -> Self
}

// Multiplication operator trait
pub trait Mul<Rhs = Self> {
    fn mul(self, rhs: Rhs) -> Self
}

// Division operator trait
pub trait Div<Rhs = Self> {
    fn div(self, rhs: Rhs) -> Self
}

// Remainder/Modulo operator trait
pub trait Rem<Rhs = Self> {
    fn rem(self, rhs: Rhs) -> Self
}

// Negation operator trait (unary -)
pub trait Neg {
    fn neg(self) -> Self
}

// Bitwise AND operator trait
pub trait BitAnd<Rhs = Self> {
    fn bitand(self, rhs: Rhs) -> Self
}

// Bitwise OR operator trait
pub trait BitOr<Rhs = Self> {
    fn bitor(self, rhs: Rhs) -> Self
}

// Bitwise XOR operator trait
pub trait BitXor<Rhs = Self> {
    fn bitxor(self, rhs: Rhs) -> Self
}

// Bitwise NOT operator trait (unary ~)
pub trait Not {
    fn not(self) -> Self
}

// Left shift operator trait
pub trait Shl<Rhs = Self> {
    fn shl(self, rhs: Rhs) -> Self
}

// Right shift operator trait
pub trait Shr<Rhs = Self> {
    fn shr(self, rhs: Rhs) -> Self
}

// Equality comparison trait (for ==)
// Uses references to avoid consuming values during comparison
pub trait PartialEq<Rhs = Self> {
    fn eq(&self, rhs: &Rhs) -> bool
}

// Less-than comparison trait (for <)
pub trait PartialLt<Rhs = Self> {
    fn lt(&self, rhs: &Rhs) -> bool
}

// Greater-than comparison trait (for >)
pub trait PartialGt<Rhs = Self> {
    fn gt(&self, rhs: &Rhs) -> bool
}

// Less-or-equal comparison trait (for <=)
pub trait PartialLe<Rhs = Self> {
    fn le(&self, rhs: &Rhs) -> bool
}

// Greater-or-equal comparison trait (for >=)
pub trait PartialGe<Rhs = Self> {
    fn ge(&self, rhs: &Rhs) -> bool
}
