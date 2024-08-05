export class Leaf {
    start: number;
    line: number;
    depth?: number = 0
    self!: number

    end?: number;
    parent?: number; // Leaf index
    constructor(start: number, line: number, depth?: number) {
        this.start = start;
        this.line = line;
        this.depth = depth;
    }
}

export class RootLeaf extends Leaf {
    file: string;
    statements: number[] = [] // Leaf indices
    constructor(start: number, line: number, file: string) {
        super(start, line);
        this.file = file;
    }
}

export class FunCallLeaf extends Leaf {
    id!: number; // Identifier Leaf index
    expressions: number[] = [] // Leaf indices
    constructor(start: number, line: number) {
        super(start, line);
    }
}
// ---------------------------
// Statements
// ---------------------------
export class FunCallStmtLeaf extends Leaf {
    id?: number; // Identifier Leaf index
    expressions : number[] = [] // Leaf indices
    constructor(start: number, line: number) {
        super(start, line);
    }
}

export class DeclarationLeaf extends Leaf {
    isMutable?: boolean = false;
    isPublic?: boolean = false;
    isNewDeclaration: boolean = false;

    declaredType?: number; // TypeExpr Leaf index
    id!: number; // Identifier Leaf index
    expression?: number // Leaf index
    constructor(start: number, line: number) {
        super(start, line);
    }
}

// ---------------------------
// Type Expressions
// ---------------------------
export class PrimaryTypeLeaf extends Leaf {
    value: string;
    constructor(start: number, line: number, value: string) {
        super(start, line);
        this.value = value;
    }
}

export class TypeChainExprLeaf extends Leaf {
    operator: 'AND' | 'OR' = 'OR'
    types : number[] = []
    constructor(start: number, line: number) {
        super(start, line);
    }
}

// ---------------------------
// Expressions
// ---------------------------
export class UnaryLeaf extends Leaf {
    operator: string;
    right!: number; // Leaf index
    constructor(start: number, line: number, operator: string) {
        super(start, line);
        this.operator = operator;
    }
}

export class BinaryLeaf extends Leaf {
    operator: string;
    left!: number; // Leaf index
    right!: number; // Leaf index
    constructor(start: number, line: number, operator: string) {
        super(start, line);
        this.operator = operator;
    }
}

export class GroupedExprLeaf extends Leaf {
    expression!: number; // Leaf index
    constructor(start: number, line: number) {
        super(start, line);
    }
}

export class IdentifierLeaf extends Leaf {
    isQualified: boolean = false
    value!: string;
    constructor(start: number, line: number, value: string) {
        super(start, line);
        this.value = value;
    }
}


export class IntLeaf extends Leaf {
    value: string;
    constructor(start: number, line: number, value: string) {
        super(start, line);
        this.value = value;
    }
}

export class FloatLeaf extends Leaf {
    value: string;
    constructor(start: number, line: number, value: string) {
        super(start, line);
        this.value = value;
    }
}
