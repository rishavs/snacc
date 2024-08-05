// Composite Pattern with Pipeline
// Define the Node Structure: Define a base structure for the nodes and specific structures for different node types.
// Define the Visitor Interface: Define a structure for the visitor with function pointers for visiting different node types.
// Implement the Visitor Functions: Implement the functions that will be called for each node type.
// Implement the Accept Method: Each node type will have an accept method that takes a visitor and calls the appropriate visit function.
// Implement the Pipeline: Create a pipeline that runs each visitor on the entire tree.
// Implementation

// export class Node {
//     start: number;
//     line: number;
//     end?: number;
//     parent?: Node;

//     constructor(start: number, line: number) {
//         this.start = start;
//         this.line = line;
//     }

//     accept(visitor: Visitor) {
//         visitor.visit(this);
//     }
// }

// export class RootNode extends Node {
//     statements: Node[] = [];

//     constructor(start: number, line: number) {
//         super(start, line);
//     }

//     accept(visitor: Visitor) {
//         visitor.visitRootNode(this);
//         this.statements.forEach(stmt => stmt.accept(visitor));
//     }
// }

// export class FunCallNode extends Node {
//     id!: IdentifierNode;
//     expressions?: Node[];

//     constructor(start: number, line: number) {
//         super(start, line);
//     }

//     accept(visitor: Visitor) {
//         visitor.visitFunCallNode(this);
//         this.expressions?.forEach(expr => expr.accept(visitor));
//     }
// }

// export class DeclarationNode extends Node {
//     isMutable?: boolean = false;
//     isPublic?: boolean = false;
//     isNewDeclaration: boolean = false;
//     declaredType?: TypeExprNode;
//     id!: IdentifierNode;
//     expression?: Node;

//     constructor(start: number, line: number) {
//         super(start, line);
//     }

//     accept(visitor: Visitor) {
//         visitor.visitDeclarationNode(this);
//         this.expression?.accept(visitor);
//     }
// }

// export interface Visitor {
//     visit(node: Node): void;
//     visitRootNode(node: RootNode): void;
//     visitFunCallNode(node: FunCallNode): void;
//     visitDeclarationNode(node: DeclarationNode): void;
// }

// export class ConcreteVisitor1 implements Visitor {
//     visit(node: Node): void {
//         // Default visit method
//     }

//     visitRootNode(node: RootNode): void {
//         console.log("Visiting RootNode with ConcreteVisitor1");
//     }

//     visitFunCallNode(node: FunCallNode): void {
//         console.log("Visiting FunCallNode with ConcreteVisitor1");
//     }

//     visitDeclarationNode(node: DeclarationNode): void {
//         console.log("Visiting DeclarationNode with ConcreteVisitor1");
//     }
// }

// export class ConcreteVisitor2 implements Visitor {
//     visit(node: Node): void {
//         // Default visit method
//     }

//     visitRootNode(node: RootNode): void {
//         console.log("Visiting RootNode with ConcreteVisitor2");
//     }

//     visitFunCallNode(node: FunCallNode): void {
//         console.log("Visiting FunCallNode with ConcreteVisitor2");
//     }

//     visitDeclarationNode(node: DeclarationNode): void {
//         console.log("Visiting DeclarationNode with ConcreteVisitor2");
//     }
// }

// export class VisitorPipeline {
//     private visitors: Visitor[] = [];

//     addVisitor(visitor: Visitor) {
//         this.visitors.push(visitor);
//     }

//     run(root: Node) {
//         this.visitors.forEach(visitor => {
//             root.accept(visitor);
//         });
//     }
// }


// const rootNode = new RootNode(0, 1);
// const funCallNode = new FunCallNode(5, 2);
// const declarationNode = new DeclarationNode(10, 3);

// rootNode.statements.push(funCallNode);
// rootNode.statements.push(declarationNode);

// const visitor1 = new ConcreteVisitor1();
// const visitor2 = new ConcreteVisitor2();

// const pipeline = new VisitorPipeline();
// pipeline.addVisitor(visitor1);
// pipeline.addVisitor(visitor2);

// pipeline.run(rootNode);