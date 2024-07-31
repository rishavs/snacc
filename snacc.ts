import { transpileFile } from "./compiler/transpiler";

let args = Bun.argv.slice(2);
if (args.length === 0) {
    console.log("Seawitch Programming Language");
    console.log("Version: 0.0.1");
    console.log("Usage: seawitch [options] <file>");
    console.log("Options:");
    console.log("  -h, --help\t\tPrint this help message");
    console.log("  -v, --version\t\tPrint version information");
} else if (args.length === 2 && args[0] === "run") {   
    let filepath = args[1];
    let res = await transpileFile(filepath);
    // process.exit(res ? 0 : 1);

    // Compile file to executable/dyn libs/ shared libs
    // Execute file 

} else {
    console.log("Invalid arguments");
}

