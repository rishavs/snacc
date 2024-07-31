import { expect, test, describe } from "bun:test";
import { Context } from "../compiler/defs";
import { genC99MainSource } from "../compiler/generator";
import { lexFile } from "../compiler/lexer";
import { parseFile } from "../compiler/parser";

describe('Transpiler', () => {
    test('Simple numbers', () => {
        let input = "let x = 10 + 11.234_5_6__7";
        let expected = /*c99*/`int main() {
    int x = 10 + 11.234567;
    return 0;
}`;
        let ctx = new Context('', input)
        lexFile(ctx)
        parseFile(ctx)

        // Check if there are any errors
        expect(ctx.errors.length).toBe(0);

        let actual = genC99MainSource(ctx.root)
        expect(expected).toBe(actual);
    })
})