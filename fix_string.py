with open("crates/ramshared-agent/src/psi.rs", "r") as f:
    code = f.read()

code = code.replace('"PSI ilegível"', '"Unreadable PSI"')

with open("crates/ramshared-agent/src/psi.rs", "w") as f:
    f.write(code)
