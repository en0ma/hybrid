use hybrid_program::ID;
use solana_instruction::Instruction;
use solana_program_test::ProgramTest;
use solana_signer::Signer;
use solana_transaction::Transaction;

async fn units_for(data: Vec<u8>) -> u64 {
    let mut context = ProgramTest::new("hybrid_program", ID, None)
        .start_with_context()
        .await;
    let ix = Instruction {
        program_id: ID,
        accounts: vec![],
        data,
    };
    let blockhash = context.get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
        &[&context.payer],
        blockhash,
    );
    let simulation = context.banks_client.simulate_transaction(tx).await.unwrap();
    if let Some(Err(err)) = simulation.result {
        panic!("simulation failed: {err:?}");
    }
    simulation
        .simulation_details
        .expect("simulation details")
        .units_consumed
}

#[tokio::test]
async fn measure_noop_cu() {
    let units = units_for(vec![0]).await;
    println!("HYBRID_CU noop {units}");
    assert!(units > 0);
}

#[tokio::test]
async fn measure_passive_quote_cu() {
    let units = units_for(vec![1]).await;
    println!("HYBRID_CU passive_quote {units}");
    assert!(units > 0);
}
