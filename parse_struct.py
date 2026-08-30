with open("crates/ramshared-tier/src/n3_state.rs", "r") as f:
    text = f.read()

count = text.count("StateTransitionError")
print(count)
