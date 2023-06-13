{$ifdef windows}{$apptype console}{$endif}
{$mode objfpc}{$H+}{$J-}{$R+}
{$overflowchecks on}{$scopedenums on}
{$modeswitch advancedrecords}

unit defs;

interface
    uses
        Generics.Collections;
    
    type FileHandle = File of char;        
    type TokenKind = (
        SOS                    // Start of file     
        , EOS                   // End of file    
        , ILLEGAL               // Illegal token    
        , SPACE                 // Space            
        , VAR_KWD               // var          
        , CONST_KWD             // const        
        , ASSIGN                // =                
        , EQUALS                // ==                
        , INT_LIT               // Integer literal  
        , FLOAT_LIT             // Float literal    
        , EXPO_LIT              // Float literal    
        , BOOL_LIT              // Bool literal     
        , HEX_LIT               // Hex literal
        , IDENTIFIER            // Identifier       

        , PLUS                  // Plus Operator       
        , INCREMENT             // Increment Operator       
    );
    type Token = record
        Kind    : TokenKind;
        Lexeme  : String;
        Cursor  : Integer;
        Line    : Integer;
    end;
    type ASLNodeKind = (
        // Special
        Not_A_Node
        
        // Non Terminals
        , Root_Node

        , Statement_Node
        , Stmt_Goggles_Node
        , Stmt_Assign_Node

        , Expression_Node
        , Expr_Assign_Node
        , Expr_Plus_Opr_Node
        
        // Terminals
        , Expr_Int_Lit_Node
        , Expr_Float_Lit_Node
        , Expr_Expo_Lit_Node
        , Expr_Bool_Lit_Node
        , Expr_Hex_Lit_Node
        
        , Identifier_Node
    );
    type NodeIdList = specialize TList<Integer>;
    type ASLNode = record
        Kind            : ASLNodeKind;
        value           : String;
        Cursor          : Integer;
        Line            : Integer;
        Depth           : Integer; 
        Parent          : Integer;
        ParentKind      : ASLNodeKind;
        Children        : NodeIdList;
    end;
    type ASList = specialize TList<ASLNode>;

    var asl: ASList;


implementation
end.