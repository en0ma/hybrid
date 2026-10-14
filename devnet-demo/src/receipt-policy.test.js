import test from "node:test";
import assert from "node:assert/strict";
import {placeData,cancelData} from "./abi.js";
import {verifyReceiptOrderLink} from "./receipt-policy.js";
test("confirmed instructions must cancel exactly the expected sequence",()=>{
 const p=placeData("ask",1,1,1,100),c=cancelData("ask",1,9);
 assert.equal(verifyReceiptOrderLink(p,c,"9").receiptSequenceLinked,true);
 assert.throws(()=>verifyReceiptOrderLink(p,c,"8"),/differs/);
 assert.throws(()=>verifyReceiptOrderLink(p,c,undefined),/required/);
 assert.throws(()=>verifyReceiptOrderLink(p,cancelData("ask",0,9),"9"),/side\/page/);
 assert.throws(()=>verifyReceiptOrderLink(p,cancelData("bid",1,9),"9"),/side\/page/);
 const zero=Uint8Array.from(c);zero.fill(0,3);
 assert.throws(()=>verifyReceiptOrderLink(p,zero,"9"),/positive/);
});
