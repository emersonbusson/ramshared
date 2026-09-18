import re

with open('drivers/block/ramshared/queue.c', 'r') as f:
    content = f.read()

content = re.sub(
    r'\tunsigned int valid_depth;\n\tint ret;\n\n\tif \(!rs_dev \|\| !parent_dev\)\n\t\treturn -EINVAL;\n\n\tvalid_depth = clamp_t\(unsigned int, q_depth, 16U, 1024U\);\n\n\tmemset\(&rs_dev->tag_set, 0, sizeof\(rs_dev->tag_set\)\);\n\trs_dev->tag_set\.ops = &ramshared_mq_ops;\n\trs_dev->tag_set\.nr_hw_queues = num_online_cpus\(\);\n\trs_dev->tag_set\.queue_depth = valid_depth;',
    '\tint ret;\n\n\tif (!rs_dev || !parent_dev)\n\t\treturn -EINVAL;\n\n\tif (q_depth < 16U || q_depth > 1024U)\n\t\treturn -EINVAL;\n\n\tmemset(&rs_dev->tag_set, 0, sizeof(rs_dev->tag_set));\n\trs_dev->tag_set.ops = &ramshared_mq_ops;\n\trs_dev->tag_set.nr_hw_queues = num_online_cpus();\n\trs_dev->tag_set.queue_depth = q_depth;',
    content
)

with open('drivers/block/ramshared/queue.c', 'w') as f:
    f.write(content)
