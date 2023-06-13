{$ifdef windows}{$apptype console}{$endif}
{$mode objfpc}{$H+}{$J-}{$R+}
{$overflowchecks on}{$scopedenums on}
{$modeswitch advancedrecords}


unit parser;

interface
    uses
        SysUtils, TypInfo, Classes, Generics.Collections,
        defs, lexer, errors;

    procedure parse_file(FileName: string);

implementation

    type ParsingContext = record
        hasErrors   : boolean;
        fp          : FileHandle;
        line        : integer;
        prevToken, currToken, nextToken: Token;

        procedure init(FileName: string);
        procedure advance();
        function parser_statement(): ASLNode;
        // procedure error_reset();
    end;


    procedure ParsingContext.advance();
    begin
        prevToken := currToken;
        currToken := nextToken;
        nextToken := lexer.get_next_token(fp, line);

        Write('{ kind: '    +  GetEnumName(TypeInfo(TokenKind), ord(currToken.Kind)));
        Write(', lexeme: '  + currToken.Lexeme);
        Write(', line: '    + IntToStr(currToken.Line));
        Write(', cursor: '  + IntToStr(currToken.Cursor));
        Writeln('}');
    end;

    function ParsingContext.parser_statement(): ASLNode;
    begin
        case currToken.Kind of
            TokenKind.ILLEGAL: begin
                raise Exception.Create('SyntaxError:IllegalToken at ( pos: '  + IntToStr(currToken.Cursor) + ', line: ' + IntToStr(currToken.Line) + ' )');
            end;
            TokenKind.SPACE: begin
                advance();
                parser_statement();
            end;
            TokenKind.EOS: begin
                raise Exception.Create('SyntaxError:IncompleteStatement at ( pos: '  + IntToStr(currToken.Cursor) + ', line: ' + IntToStr(currToken.Line) + ' )');
            end;
            // TokenKind.VAR_KWD, TokenKind.CONST_KWD: begin
            //     parse_declaration();
            // end;
            
            else begin
                raise Exception.Create('SyntaxError:UnknownStatement at ( pos: '  + IntToStr(currToken.Cursor) + ', line: ' + IntToStr(currToken.Line) + ' )');
            end;
        end;
    end;

    // procedure ParsingContext.parse_exression();
    // begin

    // end;

    // procedure ParsingContext.parse_Goggles_stmt();
    // begin
    //     if nextToken.Kind = TokenKind.SPACE then advance() end;
    //     // ParsingContext.parse_exression();
    // end;

    // procedure ParsingContext.parse_Statement();
    // begin
    //     case currToken.Kind of
    //     TokenKind.@@: begin
    //         parse_Goggles_stmt();
    //     end;
    //     else begin
    //         hasErrors := true;
    //         addErrorsToList('SyntaxError:ExpectedStatement', fp, currToken.Cursor, currToken.Line);
    //     end;
    // end;

    procedure ParsingContext.init(FileName: string);
    begin
        hasErrors := false;

        System.assign(fp, FileName);
        System.reset(fp);

        line := 1;

        // Initialize the tokens
        prevToken.Kind := TokenKind.SOS;
        prevToken.Lexeme := '';
        prevToken.Line := 1;
        prevToken.Cursor := 0;

        currToken.Kind := TokenKind.SOS;
        currToken.Lexeme := '';
        currToken.Line := 1;
        currToken.Cursor := 0;

        nextToken.Kind := TokenKind.SOS;
        nextToken.Lexeme := '';
        nextToken.Line := 1;
        nextToken.Cursor := 0;

        // Prime the context
        advance();

    end;

    procedure parse_file(FileName: string);
    var
        ctx: ParsingContext;
        node: ASLNode;
    begin
        ctx.init(FileName);
        // node := ctx.parser_statement();

        // Add node to the end of the ASL list
        Defs.asl.Add(node);


        while ctx.currToken.Kind <> TokenKind.EOS do
        begin
            ctx.advance();

            case ctx.currToken.Kind of
                TokenKind.EOS: begin
                    ctx.hasErrors := true;
                    addErrorsToList('SyntaxError:NoStatementsFound', ctx.fp, ctx.currToken.Cursor, ctx.currToken.Line);
                end;
            end;
        end;
        
        // if ctx.hasErrors then
        // begin
        //     Writeln('Errors found in file: ' + FileName);
        //     for err in errorsList do
        //     begin
        //         Writeln('Error at ( pos: '  + IntToStr(err.cursor) + ', line: ' + IntToStr(err.currLineNum) + ' ) ' + err.id);
        //     end;
        // end;

        CloseFile(ctx.fp);
        // freeAndNil(errorsList);
        // freeAndNil(ctx);
    end;
end.