{$ifdef windows}{$apptype console}{$endif}
{$mode objfpc}{$H+}{$J-}{$R+}
{$overflowchecks on}{$scopedenums on}
{$modeswitch advancedrecords}

unit lexer;

interface
    uses
        SysUtils, TypInfo, Classes,
        defs;        
    function get_next_token(var fp: FileHandle; var atLine: Integer): Token;

implementation

    function get_next_token(var fp: FileHandle; var atLine: Integer): Token;
    var
        ch      : char;
        anchor  : Integer;
    begin
        anchor          := System.filePos(fp);
        
        Result.Kind     := TokenKind.EOS;
        Result.Lexeme   := '';
        Result.Line     := atLine;
        Result.Cursor   := anchor;

        // If already at end of file, don't bother reading
        if System.eof(fp) then System.exit; 
        
        System.read(fp, ch);
        Result.Lexeme := ch;
        anchor := anchor + 1;
        
        // Handles spaces and newlines
        if ch in [#9, #10, #11, #12, #13, ' '] then begin
            Result.Kind := TokenKind.SPACE;
            Result.Lexeme := '<SPACE>';
            
            while not System.eof(fp) do begin
                System.read(fp, ch);
                anchor := anchor + 1;

                if ch in [#32, #9, #11, #12, #13 ] then begin 
                    continue;
                end else if ch = #10 then begin
                    atLine := atLine + 1;
                    Result.Line := atLine;
                    continue;
                end else begin 
                    System.seek(fp, anchor - 1);
                    break;
                end;
            end;

        // Handle Integers, floats and exponentials
        end else if ch in ['0'..'9'] then begin
            Result.Kind := TokenKind.INT_LIT;

            while not System.eof(fp) do begin
                System.read(fp, ch);
                anchor := anchor + 1;
                
                if (ch = '.') and (Result.Kind = TokenKind.INT_LIT) then begin 
                    Result.Kind := TokenKind.FLOAT_LIT;
                    Result.lexeme := Result.lexeme + ch;
                end else if ((ch = 'e') and ((Result.Kind = TokenKind.INT_LIT) or (Result.Kind = TokenKind.FLOAT_LIT))) then begin
                    Result.Kind := TokenKind.EXPO_LIT;
                    Result.lexeme := Result.lexeme + ch;
                end else if ch in ['0'..'9'] then begin
                    Result.lexeme := Result.lexeme + ch;
                    continue;
                end else if ch = '_' then begin
                    continue;
                end else begin
                    System.seek(fp, anchor - 1);
                    break;
                end;
            end;

        // Handle Keywords & identifiers
        end else if ch in ['a'..'z', 'A'..'Z', '_'] then begin
            Result.Kind := TokenKind.IDENTIFIER;

            while not System.eof(fp) do begin
                System.read(fp, ch);
                anchor := anchor + 1;

                if ch in ['a'..'z', 'A'..'Z', '0'..'9', '_'] then begin
                    Result.lexeme := Result.lexeme + ch;
                    continue;
                end else begin
                    System.seek(fp, anchor - 1);
                    break;
                end;
                
            end;
            // Writeln(Result.lexeme);
            // List all keywords here    
            case Result.lexeme of
                'var'           : Result.Kind := TokenKind.VAR_KWD;
                'true', 'false' : Result.Kind := TokenKind.BOOL_LIT;
                else            Result.Kind := TokenKind.IDENTIFIER;
            end;


        // Handle Hex literals
        end else if ch = '#' then begin
            Result.Kind := TokenKind.HEX_LIT;

            while not System.eof(fp) do begin
                System.read(fp, ch);
                anchor := anchor + 1;

                if ch in ['0'..'9', 'a'..'f', 'A'..'F'] then begin
                    Result.lexeme := Result.lexeme + ch;
                    continue;
                end else if ch = '_' then begin
                    continue;
                end else begin
                    System.seek(fp, anchor - 1);
                    break;
                end;
            end;

        // Handle equals and assigns
        end else if ch = '=' then begin
            Result.Kind := TokenKind.ASSIGN;

            while not System.eof(fp) do begin
                System.read(fp, ch);
                anchor := anchor + 1;

                if ch = '=' then begin
                    Result.Kind := TokenKind.EQUALS;
                    Result.lexeme := Result.lexeme + ch;
                    break;
                end else begin
                    System.seek(fp, anchor - 1);
                    break;
                end;
            end;

        // Handle plus and increment operator
        end else if ch = '+' then begin
            Result.Kind := TokenKind.PLUS;

            while not System.eof(fp) do begin
                System.read(fp, ch);
                anchor := anchor + 1;

                if ch = '=' then begin
                    Result.Kind := TokenKind.INCREMENT;
                    Result.lexeme := Result.lexeme + ch;
                    break;
                end else begin
                    System.seek(fp, anchor - 1);
                    break;
                end;
            end;

        end else begin
            Result.Kind := TokenKind.ILLEGAL;
            Result.Lexeme := ch;
        end;
        
    end;
end.
