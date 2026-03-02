ENTRY(_start)

PHDRS {
    text    PT_LOAD;
    rodata  PT_LOAD;
    data    PT_LOAD;
    bss     PT_LOAD;
}

SECTIONS {
    /* Limine loads kernel at higher half */
    . = 0xFFFFFFFF80000000;

    .text : {
        *(.text .text.*)
    } :text

    . = ALIGN(4096);

    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(4096);

    .data : {
        *(.data .data.*)
    } :data

    . = ALIGN(4096);

    .bss : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    } :bss

    /DISCARD/ : {
        *(.eh_frame)
        *(.note .note.*)
        *(.comment)
    }
}
