import { StreamLanguage } from "@codemirror/language";

const mnemonics = new Set(`ADC ADD AND BIT CALL CCF CP CPL DAA DEC DI EI HALT INC JP JR LD LDH NOP OR POP PUSH RES RET RETI RLA RL RLCA RLC RRA RR RRCA RRC RST SBC SCF SET SLA SRA SRL STOP SUB SWAP XOR`.split(" "));
const directives = new Set(`ALIGN ASSERT CHARMAP DB DEF DL DS DW ELSE ELIF ENDC ENDM ENDR ENDU EQU EXPORT FAIL FOR GLOBAL IF IFB IFDEF IFN IFNDEF IFNB INCLUDE INCBIN LOAD MACRO NEWCHARMAP NEXT NEXTU OPT POPO PRINTD PRINTI PRINTT PRINTV PRINTW PURGE PUSHO REDEF REPT RB RW SECTION SHIFT WARN`.split(" "));
const registers = new Set(`A B C D E H L AF BC DE HL SP`.split(" "));
const conditions = new Set(`C NC NZ Z`.split(" "));

export const lr35902Language = StreamLanguage.define({
  token(stream) {
    if (stream.eatSpace()) return null;
    if (stream.match(/;.*/)) return "comment";
    if (stream.match(/"(?:\\.|[^"\\])*"/)) return "string";
    if (stream.match(/(?:\$[\da-f]+|%[01]+|0x[\da-f]+|\d+)/i)) return "number";

    if (stream.match(/[a-z_.$@][\w.$@]*/i)) {
      const word = stream.current().toUpperCase();
      if (/^::?/.test(stream.string.slice(stream.pos))) return "def";
      if (mnemonics.has(word)) return "keyword";
      if (directives.has(word)) return "meta";
      if (registers.has(word)) return "type";
      if (conditions.has(word)) return "atom";
      return "variable";
    }

    if (stream.match(/::?|[()[\],+\-]/)) return "operator";
    stream.next();
    return null;
  },
});
