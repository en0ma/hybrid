// Pure checks linking a receipt's linked-order instructions to a known sequence.
// The expected sequence must come from the pre-placement market snapshot.
export function verifyReceiptOrderLink(placeData,cancelData,expectedSequence){
 const p=new Uint8Array(placeData),c=new Uint8Array(cancelData);
 if(p.length!==43||p[0]!==30||c.length!==11||c[0]!==31)
  throw Error("Expected linked place and cancel instruction bytes");
 if(p[1]>1||p[2]>1||c[1]!==p[1]||c[2]!==p[2])
  throw Error("Canceled order side/page does not match placement");
 const v=new DataView(c.buffer,c.byteOffset,c.byteLength);
 const sequence=v.getBigUint64(3,true);
 if(!sequence)throw Error("Canceled order sequence must be positive");
 if(expectedSequence===undefined||expectedSequence===null||expectedSequence==="")
  throw Error("Expected placement sequence is required for receipt order linkage");
 const expected=BigInt(expectedSequence);
 if(expected<=0n||expected>=(1n<<64n))throw Error("Expected sequence outside u64 range");
 if(sequence!==expected)throw Error("Canceled sequence differs from expected placed order sequence");
 return {sequence:sequence.toString(),side:p[1]===0?"ask":"bid",pageIndex:p[2],receiptSequenceLinked:true};
}
