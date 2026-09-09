with open('crates/ramshared-block/src/handshake.rs', 'r') as f:
    content = f.read()
import re
content = re.sub(r'<<<<<<< HEAD\n(.*?)\n=======\n(.*?)\n>>>>>>> [a-f0-9]+ .*?\n', r'\2\n', content, flags=re.DOTALL)
with open('crates/ramshared-block/src/handshake.rs', 'w') as f:
    f.write(content)
