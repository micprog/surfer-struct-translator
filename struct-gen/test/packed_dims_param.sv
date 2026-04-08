function automatic int unsigned idx_width(input int unsigned num_idx);
  if (num_idx > 32'd1) begin
    return $unsigned($clog2(num_idx));
  end else begin
    return 32'd1;
  end
endfunction

module packed_dims_param_top #(
  parameter int unsigned NrVFUs = 7,
  parameter int unsigned MaxVInsnQueueDepth = 8
) ();
  typedef struct packed {
    logic [NrVFUs-1:0][idx_width(MaxVInsnQueueDepth + 1)-1:0] insn_queue_cnt_q;
    logic [NrVFUs-1:0] insn_queue_done;
  } sequencer_probe_t /* public */;

  sequencer_probe_t probe_q;
endmodule
