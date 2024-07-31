import { expect, test, describe } from "bun:test";
import { Context } from "./compiler/defs";
import { genC99Files, genC99MainSource } from "./compiler/generator";
import { lexFile } from "./compiler/lexer";
import { parseFile } from "./compiler/parser";

describe('Transpiler', () => {

    test('Simple declaration', () => {
        let input = "let x = 10";
        let expected = /*c99*/`int main() {
    int x = 10;
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

    test('Simple function call', () => {
        let input = "print(10)";
        let expected = /*c99*/`int main() {
    print(10);
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

    test('Simple expression', () => {
        let input = "let x = 10 + 20";
        let expected = /*c99*/`int main() {
    int x = 10 + 20;
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

    test('Simple function call with expression', () => {
        let input = "print(10 + 20)";
        let expected = /*c99*/`int main() {
    print(10 + 20);
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

    test('Simple expression with function call', () => {
        let input = "let x = 10 + print(20)";
        let expected = /*c99*/`int main() {
    int x = 10 + print(20);
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

    test('Simple expression with function call and identifier', () => {
        let input = "let x = 10 + print(y)";
        let expected = /*c99*/`int main() {
    int x = 10 + print(y);
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

    test('Complelx expression with Binray, unary & grouping', () => {
        let input = "let x = 10 + -3 * (z / (2 + 5))";
        let expected = /*c99*/`int main() {
    int x = 10 + -3 * (z / (2 + 5));
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
});
