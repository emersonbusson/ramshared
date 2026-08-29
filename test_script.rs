fn main() {
    let mut q: std::collections::VecDeque<i32> = std::collections::VecDeque::new();
    q.reserve(5);
    println!("capacity: {}", q.capacity());
}
