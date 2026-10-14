//! Own-ACK Native control custody settlement. Durable/project settlement remains closed.

use super::*;

#[cfg(test)]
#[path = "mirror_tests.rs"]
mod tests;

pub(crate) fn reconcile(engine: &AudioEngine, ticket: &PadPairTicket) -> PyResult<bool> {
    let state = &ticket.state;
    state.belongs_to(engine)?;
    let Some(ack) = state.guard.native_acknowledgement() else {
        return Ok(false);
    };
    super::super::super::resident_relocation::reconcile(engine)?;
    let requests = engine
        .pad_request_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("request lock poisoned"))?;
    if state.mirrored.load(Ordering::Acquire) {
        return Ok(true);
    }
    let original = state.snapshots();
    let [source, target] = original.map(|snapshot| snapshot.slot());
    let ownership = &state.guard.ownership;
    let after = [
        ownership.slot_snapshot(source),
        ownership.slot_snapshot(target),
    ];
    let [Some(source_after), Some(target_after)] = after else {
        return Ok(false);
    };
    let after = [source_after, target_after];
    if !ack.source_after.matches(source_after) || !ack.target_after.matches(target_after) {
        return Err(PyRuntimeError::new_err(
            "own pair ACK placement changed; settlement remains fenced",
        ));
    }
    // Allocate independent empty state and acquire every fallible lock before claim.
    let mut canonical_empty = ResidentStemCache::default();
    let loading = engine
        .loading_sample_ids
        .lock()
        .map_err(|_| PyRuntimeError::new_err("loading lock poisoned"))?;
    let mut cache = engine
        .sample_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("sample cache lock poisoned"))?;
    let mut generations = engine
        .loaded_source_generations
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source generation lock poisoned"))?;
    let mut digests = engine
        .loaded_source_digests
        .lock()
        .map_err(|_| PyRuntimeError::new_err("source digest lock poisoned"))?;
    let mut leases = engine
        .cold_leases
        .lock()
        .map_err(|_| PyRuntimeError::new_err("cold custody lock poisoned"))?;
    let mut intents = engine
        .timing_intents
        .lock()
        .map_err(|_| PyRuntimeError::new_err("timing intent lock poisoned"))?;
    let mut records = engine
        .current_constant_timing
        .lock()
        .map_err(|_| PyRuntimeError::new_err("current timing lock poisoned"))?;
    let mut resident = engine
        .resident_stem_cache
        .lock()
        .map_err(|_| PyRuntimeError::new_err("resident custody lock poisoned"))?;
    let mut history = engine
        .cold_pcm_history
        .lock()
        .map_err(|_| PyRuntimeError::new_err("PCM history lock poisoned"))?;
    let current_samples = original.map(|before| cache[before.slot()].clone());
    let mut runtime = match (&engine.input_runtime, &state.runtime) {
        (Some(runtime), Some(captured)) => Some(
            runtime
                .prepare_pair_mirror(captured, after, &current_samples)
                .map_err(PyRuntimeError::new_err)?,
        ),
        (None, None) => None,
        _ => return Err(PyRuntimeError::new_err("pair MIDI runtime owner changed")),
    };
    // Locks pin these exact current control rows. Prepare all comparisons and Arc
    // clones before the marker; the claim checks only fixed Native state.
    let current_generations = original.map(|before| generations[before.slot()]);
    let current_intents = original.map(|before| intents[before.slot()]);
    let control_current = original.iter().enumerate().all(|(index, before)| {
        let slot = before.slot();
        let custody = &state._custody[index];
        let source_matches = match (&custody._sample, &cache[slot]) {
            (Some(captured), Some(current)) => captured.same_source(current),
            (None, None) => true,
            _ => false,
        };
        let lease_matches = match (&custody._lease, &leases[slot]) {
            (Some(captured), Some(current)) => {
                captured.assignment_id() == current.assignment_id() && !current.assignment_retired()
            }
            (None, None) => true,
            _ => false,
        };
        let original_selection =
            committed_stem_residency::capture_pair_selection_fences(&resident[slot]);
        let Some(proposals) = engine
            .resident_proposals
            .capture_pair_fences_for_cell(before.cell())
        else {
            return false;
        };
        let owners_match = state.guard.participants[index].selection_fences.len()
            == original_selection.len() + proposals.len()
            && original_selection
                .iter()
                .chain(&proposals)
                .zip(&state.guard.participants[index].selection_fences)
                .all(|((current, _), (captured, _))| Arc::ptr_eq(current, captured));
        let records_owned = records[slot].iter().all(|record| {
            record.content_key.is_some_and(|key| {
                key.belongs_to(ownership.owner_identity()) && key.cell() == before.cell()
            })
        });
        requests[slot] == state.captured_requests[index]
            && !loading.contains(&slot)
            && engine.cold_loading[slot].load(Ordering::Acquire) == 0
            && source_matches
            && lease_matches
            && owners_match
            && records_owned
            && digests[slot] == custody.digest
            && engine.cold_lease_generations[slot].load(Ordering::Acquire)
                == if leases[slot].is_some() {
                    generations[slot].0
                } else {
                    0
                }
            && Arc::ptr_eq(
                &state.guard.participants[index].preparation_epoch,
                &engine.prepared_source_epochs[before.cell().index()],
            )
    });
    let write = match ownership.prepare_slot_write_fixed(source_after, source_after.lifetime()) {
        Ok(write) => write,
        Err(PlacementError::Pending) => return Ok(false),
        Err(error) => return Err(PyRuntimeError::new_err(error.to_string())),
    };
    let mut retired_records = Vec::new();
    let mut retired_history = Vec::new();
    let runtime_bindings = runtime
        .as_ref()
        .map(|runtime| runtime.publication_bindings());
    let outcome = ownership.commit_slot_write_fixed(
        &write,
        || {
            control_current
                && ack.source_after.matches(source_after)
                && ack.target_after.matches(target_after)
                && ownership.slot_snapshot_fields_current(source_after)
                && ownership.slot_snapshot_fields_current(target_after)
                && runtime_bindings.is_none_or(|bindings| {
                    after
                        .iter()
                        .zip(bindings)
                        .all(|(snapshot, (loaded, binding))| {
                            !loaded
                                || binding.is_some_and(|binding| {
                                    binding.placement == Some(*snapshot)
                                        && ownership.current(snapshot.slot(), binding)
                                })
                        })
                })
                && original.iter().enumerate().all(|(index, before)| {
                    let destination = after[1 - index];
                    ownership.pair_mirror_source_current(
                        *before,
                        destination,
                        current_samples[index].as_ref(),
                        current_generations[index],
                        current_intents[index],
                        state.guard.reservation,
                    )
                })
        },
        |_| {
            cache.swap(source, target);
            generations.swap(source, target);
            digests.swap(source, target);
            leases.swap(source, target);
            intents.swap(source, target);
            records.swap(source, target);
            resident.swap(source, target);
            history.swap(source, target);
            let source_lease_generation =
                engine.cold_lease_generations[source].load(Ordering::Acquire);
            let target_lease_generation =
                engine.cold_lease_generations[target].load(Ordering::Acquire);
            engine.cold_lease_generations[source].store(target_lease_generation, Ordering::Release);
            engine.cold_lease_generations[target].store(source_lease_generation, Ordering::Release);
            if source_after.lifetime() == 0 {
                generations[source] = (0, 0);
                intents[source] = TimingIntent::Legacy;
                std::mem::swap(&mut resident[source], &mut canonical_empty);
                std::mem::swap(&mut records[source], &mut retired_records);
                std::mem::swap(&mut history[source], &mut retired_history);
            }
            if let Some(runtime) = &mut runtime {
                runtime.apply();
            }
            // Existing admitted workers observe Pending until all custody views agree.
            ownership.publish_pair_control_slots(&after);
            state.mirrored.store(true, Ordering::Release);
        },
    );
    match outcome {
        Ok(_) => Ok(true),
        Err(PlacementError::Pending) => Ok(false),
        Err(error) => Err(PyRuntimeError::new_err(format!(
            "pair control mirror remains fenced: {error}"
        ))),
    }
}
