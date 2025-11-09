#ifndef ASMDEFS_H_
#define ASMDEFS_H_

#define PTR_SIZE 8

#define ENTRY(name) ENTRY_ALIGN(name, 1)

#define ENTRY_ALIGN(name, alignement) \
        .global name;                 \
        .type name, @function;        \
        .align alignement;            \
name:

#define END(name) .size name, .- name;

#define ENDPROC(name)          \
        .type name, @function; \
        END(name)

#endif // ASM_DEFS_H_
