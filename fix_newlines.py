with open("crates/ramshared-agent/src/psi.rs", "r") as f:
    code = f.read()

search_s = 'let s = "some avg10=1.23 avg60=4.56 avg300=7.89 total=999 some other=1\n";'
search_p_s = 'let s = "some avg10=1.23 avg60=4.56 avg300=7.89 total=999 some other=1\\n";'

search_s2 = 'let s2 = "some avg10=1.23 avg60=4.56 avg300=7.89 total=999 full avg10=0 total=0\n";'
search_p_s2 = 'let s2 = "some avg10=1.23 avg60=4.56 avg300=7.89 total=999 full avg10=0 total=0\\n";'

search_s3 = 'let s = "some total=999 avg10=1.23 avg60=4.56 avg300=7.89\n";'
search_p_s3 = 'let s = "some total=999 avg10=1.23 avg60=4.56 avg300=7.89\\n";'

code = code.replace(search_s, search_p_s)
code = code.replace(search_s2, search_p_s2)
code = code.replace(search_s3, search_p_s3)

with open("crates/ramshared-agent/src/psi.rs", "w") as f:
    f.write(code)
