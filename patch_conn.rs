<<<<<<< SEARCH
                let err = match w.write_all(&r.reply) {
                    Ok(_) => {
                        if !r.data.is_empty() {
                            w.write_all(&r.data).err()
                        } else {
                            None
                        }
                    }
                    Err(e) => Some(e),
                };

                let err = match err {
                    None => w.flush().err(),
                    Some(e) => Some(e),
                };

                if let Some(e) = err {
                    attempt += 1;
                    if attempt > 5 {
                        eprintln!("[ramsharedd] conn: write failed after retries: {}", e);
                        break;
                    }
                    if e.kind() != std::io::ErrorKind::WouldBlock {
                        eprintln!("[ramsharedd] conn: fatal write error: {}", e);
                        break;
                    }
                    let jitter = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros()
                        % 20) as u64;
                    let jitter_millis = jitter * backoff.as_millis() as u64 / 100;

                    std::thread::sleep(backoff + std::time::Duration::from_millis(jitter_millis));
                    backoff = std::cmp::min(backoff * 2, max_backoff);
                } else {
                    break;
                }
=======
                let err = match w.write_all(&r.reply) {
                    Ok(_) if !r.data.is_empty() => w.write_all(&r.data).err(),
                    Ok(_) => None,
                    Err(e) => Some(e),
                };

                let err = err.or_else(|| w.flush().err());

                let Some(e) = err else {
                    break; // Success!
                };

                attempt += 1;
                if attempt > 5 {
                    eprintln!("[ramsharedd] conn: write failed after retries: {}", e);
                    break;
                }

                if e.kind() != std::io::ErrorKind::WouldBlock {
                    eprintln!("[ramsharedd] conn: fatal write error: {}", e);
                    break;
                }

                let jitter = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros()
                    % 20) as u64;
                let jitter_millis = jitter * backoff.as_millis() as u64 / 100;

                std::thread::sleep(backoff + std::time::Duration::from_millis(jitter_millis));
                backoff = std::cmp::min(backoff * 2, max_backoff);
>>>>>>> REPLACE
