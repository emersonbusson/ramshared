import re

with open("crates/ramshared-agent/src/watchdog.rs", "r") as f:
    code = f.read()

# Modify touch
code = re.sub(
    r"    pub fn touch\(&mut self, now: Instant\) {\n        if now >= self\.last {\n            self\.last = now;\n        }\n    }",
    r"    pub fn touch(&mut self, now: Instant) {\n        if now < self.last {\n            return;\n        }\n        self.last = now;\n    }",
    code
)

# Modify expired
code = re.sub(
    r"    pub fn expired\(&self, now: Instant\) -> bool {\n        if now < self\.last {\n            return false;\n        }\n        now\.saturating_duration_since\(self\.last\) >= self\.deadline\n    }",
    r"    pub fn expired(&self, now: Instant) -> bool {\n        if now < self.last {\n            return false;\n        }\n        now.saturating_duration_since(self.last) >= self.deadline\n    }",
    code
)

# Modify check
code = re.sub(
    r"    pub fn check\(&self, now: Instant\) -> Result<\(\), WatchdogError> {\n        if self\.expired\(now\) {\n            Err\(WatchdogError::HeartbeatTimeout\(self\.deadline\)\)\n        } else {\n            Ok\(\(\)\)\n        }\n    }",
    r"    pub fn check(&self, now: Instant) -> Result<(), WatchdogError> {\n        if self.expired(now) {\n            return Err(WatchdogError::HeartbeatTimeout(self.deadline));\n        }\n        Ok(())\n    }",
    code
)

with open("crates/ramshared-agent/src/watchdog.rs", "w") as f:
    f.write(code)
