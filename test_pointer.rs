fn foo<T>(val: *mut T) {}
fn main() {
    let mut x = 5;
    foo(&mut x);
}
