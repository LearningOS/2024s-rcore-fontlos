set shell := ["nu", "-c"]

# LOG
export LOG := ""

run: build
    qemu-system-riscv64 -machine virt -nographic -bios ./bootloader/rustsbi-qemu.bin -device loader,file="os/target/riscv64gc-unknown-none-elf/release/os.bin",addr=0x80200000

build: kernel
	rust-objcopy --binary-architecture=riscv64 "os/target/riscv64gc-unknown-none-elf/release/os" --strip-all -O binary "os/target/riscv64gc-unknown-none-elf/release/os.bin"

kernel: user
    cargo build --release

user:
    #!nu
    cd ./user
    #cargo clean
    rm -rf ./build
    mkdir ./build/bin/
    mkdir ./build/elf/
    mkdir ./build/app/
    mkdir ./build/asm/
    cd ..
    let chapter = (git rev-parse --abbrev-ref HEAD | parse 'ch{id}').id.0 | into int
    let test = $chapter
    let base = 1
    if $chapter == 1 {
        let apps = (ls ./user/src/bin).name
    } else {
        if $base == 0 {
            for $id in $base..$test {
                ls ./user/src/bin | each { |$f|
                    if ($f.name | str starts-with $"user\\src\\bin\\ch($id)_") {
                        cp $f.name './user/build/app'
                    }
                }
            }
        } else if $base == 1 {
            for $id in $base..$test {
                ls ./user/src/bin | each { |$f|
                    if ($f.name | str starts-with $"user\\src\\bin\\ch($id)b_") {
                        cp $f.name './user/build/app'
                    }
                }
            }
        } else {
            for $id in $base..$test {
                ls ./user/src/bin | each { |$f|
                    if ($f.name | str starts-with $"user\\src\\bin\\ch($id)") {
                        cp $f.name './user/build/app'
                    }
                }
            }
        }
    }