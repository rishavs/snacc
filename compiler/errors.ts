export class UnknownError extends Error {
    constructor(pos: number, line: number) {
        super(`Found an error at line ${line}, pos ${pos}`)
        this.name = "UnknownError"
        this.cause = "Please report this error to the developer"

        // Modify the stack trace to exclude the constructor call
        if (Error.captureStackTrace) {
            Error.captureStackTrace(this, UnknownError);
        }
    }   
}

export class IllegalCharError extends Error {
    constructor(c: string, pos: number, line: number) {
        super(`Found an unexpcted "${c}" at line ${line}, pos ${pos}`)
        this.name = "IllegalCharacterError"

        // Modify the stack trace to exclude the constructor call
        if (Error.captureStackTrace) {
            Error.captureStackTrace(this, IllegalCharError);
        }
    }   
}
export class UnclosedDelimiterError extends Error {
    constructor(syntax: string, delimiter: string, pos: number, line: number) {
        let msg = `The delimiter "${delimiter}" for ${syntax} at line ${line}, pos ${pos} is not closed`
        super(msg)
        this.name = "UnclosedDelimiterError"

        // Modify the stack trace to exclude the constructor call
        if (Error.captureStackTrace) {
            Error.captureStackTrace(this, UnclosedDelimiterError);
        }
    }   
}


export class MissingSyntaxError extends Error {
    constructor(expectedSyntax: string, at: number, line: number, found?: string, ) {
        let expected = `Expected ${expectedSyntax} at ${line}:${at} ,`
        let got = found
            ? `but instead found "${found}"` 
            : `but instead reached end of input`
        super(expected + got);
        this.name = "Syntax Error";
        
        // Modify the stack trace to exclude the constructor call
        if (Error.captureStackTrace) {
            Error.captureStackTrace(this, MissingSyntaxError);
        }
    }
}

export class MissingSpecificTokenError extends Error {
    constructor(expectedSyntax: string, expectedTokenKind: string, at: number, line: number, found?: string) {
        let expected = `Expected ${expectedTokenKind} for ${expectedSyntax} at ${line}:${at},`
        let got = found
            ? `but instead found "${found}"` 
            : `but instead reached end of input`
        super(expected + got);
        this.name = "Syntax Error";
        
        // Modify the stack trace to exclude the constructor call
        if (Error.captureStackTrace) {
            Error.captureStackTrace(this, MissingSpecificTokenError);
        }
    }
}