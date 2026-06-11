# Designed to be run in side the c64-kernal-mod directory

rm build/modifykernal*
ca65 modifykernal.asm -o build/modifykernal.o
ld65 -C c64.cfg build/modifykernal.o -o build/modifykernal.prg
ls -l build/modifykernal.prg