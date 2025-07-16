import json
import matplotlib.pyplot as plt
import numpy as np
from textwrap import fill

def load_experiments(file_path):
  """Load ndjson file into a list of experiment dicts"""
  experiments = []
  with open(file_path) as f:
    for line in f:
      try:
        experiments.append(json.loads(line))
      except json.JSONDecodeError as e:
        print(f"Skipping malformed line: {e}")
  return experiments

def plot_averaged_times(experiments, output_file="../data/time_vs_number_of_nodes.png"):
  """Plot averaged total_level_time vs nodes_searched"""
  plt.figure(figsize=(12, 8))

  for exp in experiments:
    exp_id = exp["metadata"]["experiment_id"]

    for i, (server1, server2) in enumerate(exp["server_side"]):
      nodes = server1["nodes_searched"]
      avg_time = (server1["total_level_time"] + server2["total_level_time"]) / 2

      plt.plot(
          nodes,
          avg_time,
          marker='o' if i == 0 else 's',  # Different marker for first level
          linestyle='-',
          label=f"Level {i+1}"
      )

  plt.xlabel("Nodes Searched")
  plt.ylabel("Average Server Search Time")
  plt.title("RideAustin Busiest Day - Averaged Server Performance")
  plt.grid(True)

  # Parameter annotation
  sample_params = experiments[0]["parameters"]
  param_text = "\n".join([
    f"Parameters:",
    f"Clients: {sample_params['num_clients']}",
    f"Dimensions: {sample_params['dimensions']}",
    f"Threshold: {sample_params['threshold']}",
    f"Ball Radius: {sample_params['ball_radius']}"
  ])

  plt.annotate(
      param_text,
      xy=(0.98, 0.5),
      xycoords='axes fraction',
      ha='right',
      va='center',
      bbox=dict(boxstyle='round', alpha=0.2))

  plt.legend(bbox_to_anchor=(1.05, 1), loc='upper left')
  plt.tight_layout()
  plt.savefig(output_file, dpi=300, bbox_inches='tight')
  print(f"Saved averaged plot to {output_file}")


def plot_stacked_time_components(experiments, output_file="../data/stacked_time_components.png"):
  """Stacked bar plot where each bar is a level, colored by time components"""
  plt.figure(figsize=(14, 8))

  # Prepare data structure
  components = ['FSS', 'GCequality', 'FieldActions', 'GCCompare']
  colors = ['#1f77b4', '#ff7f0e', '#2ca02c', '#d62728']
  level_names = []
  component_data = {comp: [] for comp in components}

  for exp in experiments:
    exp_id = exp["metadata"]["experiment_id"]
    for i, (server1, server2) in enumerate(exp["server_side"]):
      # Average the two servers' times
      level_names.append(f"{exp_id}\nL{i+1}")
      for comp in components:
        avg_time = (server1["time_breakdown"][comp] + server2["time_breakdown"][comp]) / 2
        component_data[comp].append(avg_time)

  # Create stacked bars
  x = np.arange(len(level_names))
  bottom = np.zeros(len(level_names))

  for comp, color in zip(components, colors):
    plt.bar(x, component_data[comp], width=0.8, bottom=bottom,
            label=comp, color=color, edgecolor='white')
    bottom += component_data[comp]

  # Formatting
  plt.xlabel('Level')
  plt.ylabel('Time (s)')
  plt.title('Time Breakdown by Level')
  plt.xticks(x, level_names, rotation=45, ha='right')

  # Add legend and parameter info
  plt.legend(title='Components', bbox_to_anchor=(1.05, 1), loc='upper left')

  sample_params = experiments[0]["parameters"]
  param_text = "\n".join([
    f"Parameters:",
    f"Clients: {sample_params['num_clients']}",
    f"Dimensions: {sample_params['dimensions']}",
    f"Threshold: {sample_params['threshold']}",
    f"Ball Radius: {sample_params['ball_radius']}"
  ])

  plt.annotate(
      param_text,
      xy=(0.98, 0.5),
      xycoords='axes fraction',
      ha='right',
      va='center',
      bbox=dict(boxstyle='round', alpha=0.2))

  plt.grid(True, axis='y')
  plt.tight_layout()
  plt.savefig(output_file, dpi=300, bbox_inches='tight')
  print(f"Saved stacked component plot to {output_file}")

if __name__ == "__main__":
  experiments = load_experiments("../data/ride_austin_experiments.json")
  if experiments:
    plot_averaged_times(experiments)
    plot_stacked_time_components(experiments)
  else:
    print("No valid experiments found in the log file.")