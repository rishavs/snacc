import { Token } from "./lox";
import { digit, eos, is, letter, ParsingState, ws, type LastParsed} from "./nippy";
import { expect, test, describe } from "bun:test";

describe('Testing Parser "eos"', () => {
    test("Null input", () => {
        let p = new ParsingState('')
        let check = eos()
        let lastParsed = {
            ok: true,
            name: 'eos',
            kind: 'eos',
            value: '',
        } as LastParsed
        let expected = new ParsingState('')
        expected.parsed = lastParsed
        expected.i = 0
        expected.line = 0
        
        expect(check(p)).toEqual(expected);
    });
    test("Any input", () => {
        let p = new ParsingState('xyz')
        let check = eos()
        expect(check()).toBe(null);
    });
});

describe('Testing Parser "is"', () => {
    test ("Empty input", () => {
        let p = new ParsingState('')
        let check = is('let')
        expect(check()).toBe(null);
    });

    test("Match", () => {
        let p = new ParsingState('let xyz = 3')
        let check = is('let')
        let expected = new Token('is', 'let', 0, 0)
        expect(check()).toEqual(expected);
    });

    test("No match", () => {
        let p = new ParsingState('let xyz = 3')
        let check = is('xyz')

        expect(check()).toBe(null);
    });

    test("No partial match", () => {
        let p = new ParsingState('let xyz = 3')
        let check = is('letx')
        expect(check()).toBe(null);
    });

    test("Match at specific position", () => {
        let p = new ParsingState('xyz let')
        let check = is('let')
        i = 4 // Simulate internal pointer after 'xyz '
        expect(check()).toEqual(new Token('is', 'let', 4, 0));
    });
});

// digit() tests
describe('Testing Parser "digit"', () => {
    test ("Empty input", () => {
        let p = new ParsingState('')
        let check = digit()
        expect(check()).toBe(null);
    });

    test("Match", () => {
        let p = new ParsingState('3abc')
        let check = digit()
        let expected = new Token('digit', '3', 0, 0)
        expect(check()).toEqual(expected);
    });

    test("No match", () => {
        let p = new ParsingState('abc')
        let check = digit()
        expect(check()).toBe(null);
    });

    test("Match at specific position", () => {
        let p = new ParsingState('abc3')
        let check = digit()
        i = 3 // Simulate internal pointer after 'abc'
        let expected = new Token('digit', '3', 3, 0)
        expect(check()).toEqual(expected);
    });

    test ("Multiple digits", () => {
        let p = new ParsingState('123abc')
        let check = digit()
        let expected = new Token('digit', '1', 0, 0)
        expect(check()).toEqual(expected);
        expected = new Token('digit', '2', 1, 0)
        expect(check()).toEqual(expected);
        expected = new Token('digit', '3', 2, 0)
        expect(check()).toEqual(expected);
    });
});

// letter() tests
describe('Testing Parser "letter"', () => {
    test("Empty input", () => {
        let p = new ParsingState('')
        let check = letter()
        expect(check()).toBe(null);
    });

    test("Match", () => {
        let p = new ParsingState('a1')
        let check = letter()
        let expected = new Token('letter', 'a', 0, 0)
        expect(check()).toEqual(expected);
    });

    test("No match", () => {
        let p = new ParsingState('1a')
        let check = letter()
        expect(check()).toBe(null);
    });

    test("Match at specific position", () => {
        let p = new ParsingState('1a')
        let check = letter()
        i = 1 // Simulate internal pointer after '1'
        let expected = new Token('letter', 'a', 1, 0)
        expect(check()).toEqual(expected);
    });

    test("Multiple letters", () => {
        let p = new ParsingState('abc')
        let check = letter()
        let expected = new Token('letter', 'a', 0, 0)
        expect(check()).toEqual(expected);
        expected = new Token('letter', 'b', 1, 0)
        expect(check()).toEqual(expected);
        expected = new Token('letter', 'c', 2, 0)
        expect(check()).toEqual(expected);
    });
});

describe('Testing Parser "ws"', () => {
    test("Empty input", () => {
        let p = new ParsingState('')
        let check = ws()
        expect(check()).toBe(null);
    });

    test("Match space", () => {
        let p = new ParsingState(' \t\nabc')
        let check = ws()
        let expected = new Token('ws', '', 0, 0)
        expect(check()).toEqual(expected);
    });

    test("Match tab", () => {
        let p = new ParsingState('\tabc')
        let check = ws()
        let expected = new Token('ws', '', 0, 0)
        expect(check()).toEqual(expected);
    });

    test("Match newline", () => {
        let p = new ParsingState('\nabc')
        let check = ws()
        let expected = new Token('ws', '', 0, 1)
        expect(check()).toEqual(expected);
    });

    test("No match", () => {
        let p = new ParsingState('abc')
        let check = ws()
        expect(check()).toBe(null);
    });

    test("Match at specific position", () => {
        let p = new ParsingState('abc \t\n')
        let check = ws()
        i = 3 // Simulate internal pointer after 'abc'
        let expected = new Token('ws', '', 3, 0)
        expect(check()).toEqual(expected);
    });

    test("Multiple whitespaces", () => {
        let p = new ParsingState(' \t\nabc')
        let check = ws()
        let expected = new Token('ws', '', 0, 0)
        expect(check()).toEqual(expected);
    });
});