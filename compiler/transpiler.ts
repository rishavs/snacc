import { Context } from './defs'
import { lexFile } from './lexer'
import { parseFile } from './parser';

export const transpileFile = async (filepath: string) => {
    const file = Bun.file(filepath);
    if (! await file.exists()) {
        console.error("Error: File not found");
    }
    // Check if file is a seawitch file *.sea
    if (!filepath.endsWith(".sea")) {
        console.error("Error: Invalid file type. Seawitch files must have a .sea extension");
    }

    // Transpile file
    let ctx = new Context(filepath, await file.text())

    // Lexing
    lexFile(ctx)

    // Parsing
    if (ctx.errors.length > 0) {
        console.error("Error: Lexing failed");
        console.error(ctx.errors)

        return false
    }

    parseFile(ctx)

    // if (ctx.errors.length > 0) {
    //     console.error("Error: Parsing failed");
    //     console.error(ctx.errors)

    //     return false
    // }
    console.log(ctx.errors)
    console.log(ctx.tokens)
    console.log("AST", JSON.stringify(ctx.root, [
        'tokens', 'name', 'start', 'line', 'end', 'errors', 't', 'root', 'type',
        'kind', 'operator', 'value', 'isFloat', 'isQualified', 'isNewDeclaration', 
        'isPublic', 'isMutable', 
        'statements', 'left', 'right', 'identifier', 'expression'
    ], 4))
    // console.log(ctx)
    return true
}
