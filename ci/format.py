from common import run, done

if __name__ == "__main__":
    print("Format", flush=True)
    run("cargo fmt --all -- --check")
    done()
