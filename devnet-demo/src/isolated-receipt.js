// Ensure one lifecycle receipt cannot disguise additional Hybrid calls.
export function selectIsolatedHybridInstruction(instructions,programId,allowedOpcodes,decode){
 const hits=instructions.filter(ix=>ix.programId?.equals(programId));
 if(hits.length!==1)throw Error("Receipt must contain exactly one top-level Hybrid instruction");
 const ix=hits[0];
 if(typeof ix.data!=="string")throw Error("Hybrid instruction lacks encoded data");
 const data=decode(ix.data);
 if(!data?.length||!allowedOpcodes.has(data[0]))
  throw Error("Hybrid receipt instruction opcode does not match lifecycle stage");
 return {instruction:ix,data};
}
